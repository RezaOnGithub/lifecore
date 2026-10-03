use crate::{
    AppError, ErrorObject,
    proposal::{self, DecodeError, Format, Proposal},
    service::{ProposalRequest, Service},
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, MapAccess, Visitor},
};
use serde_json::{Value, value::RawValue};
use std::{collections::BTreeMap, fmt};

//A A restricted single-call JSON-RPC 2.0 profile: every request has a non-null
//A ID and gets exactly one response. Batches and notifications are rejected
//A before dispatch.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Result(Value),
    Error(ErrorObject),
}

impl Response {
    fn new(id: Value, outcome: Outcome) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            outcome,
        }
    }
    pub fn is_success(&self) -> bool {
        matches!(self.outcome, Outcome::Result(_))
    }
}

fn protocol_error(code: i64, message: impl Into<String>) -> ErrorObject {
    ErrorObject {
        code,
        message: message.into(),
        data: None,
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeRequest {
    pub text: String,
    pub format: Format,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodeRequest<P = Proposal> {
    pub proposal: P,
    pub format: Format,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encoded {
    pub text: String,
}

pub fn decode_proposal(request: DecodeRequest) -> Result<Proposal, AppError> {
    proposal::decode(&request.text, request.format).map_err(AppError::MalformedProposal)
}

pub fn encode_proposal(request: EncodeRequest) -> Result<Encoded, AppError> {
    proposal::encode(&request.proposal, request.format)
        .map(|text| Encoded { text })
        .map_err(|message| AppError::Internal { message })
}

//A A `proposal` parameter is decoded on its own, from its raw text, so its
//A failures are malformed proposals located by a path inside it, as for a
//A change file, and repeated payload fields are still seen. Line and column
//A are dropped because they would count from the member, not the request;
//A serde_json's message text still names that member-relative position.
fn embedded(proposal: &RawValue) -> Result<Proposal, AppError> {
    proposal::decode(proposal.get(), Format::Json).map_err(|error| {
        AppError::MalformedProposal(DecodeError {
            line: None,
            column: None,
            ..error
        })
    })
}

fn proposal_request(request: ProposalRequest<Box<RawValue>>) -> Result<ProposalRequest, AppError> {
    Ok(ProposalRequest {
        proposal: embedded(&request.proposal)?,
        evaluation_time: request.evaluation_time,
        now: request.now,
    })
}

enum DispatchError {
    MethodNotFound,
    Application(AppError),
}

impl From<AppError> for DispatchError {
    fn from(error: AppError) -> Self {
        Self::Application(error)
    }
}

fn call<P: DeserializeOwned, R: Serialize>(
    params: &str,
    operation: impl FnOnce(P) -> Result<R, AppError>,
) -> Result<Value, DispatchError> {
    let params = serde_json::from_str(params).map_err(|error| AppError::InvalidParams {
        message: error.to_string(),
    })?;
    let result = operation(params)?;
    serde_json::to_value(result).map_err(|error| {
        AppError::Internal {
            message: error.to_string(),
        }
        .into()
    })
}

//A Method names are provisional; the UI stages reshape them (PLAN 4.3).
//A `init` and filesystem path selection are deliberately absent.
fn dispatch(service: &Service, method: &str, params: &str) -> Result<Value, DispatchError> {
    match method {
    "database.describe" => call(params, |p| service.describe(p)),
    "entries.list" => call(params, |p| service.list_entries(p)),
    "entries.get" => call(params, |p| service.get_entry(p)),
    "plan_id.mint" => call(params, |p| service.mint_plan_id(p)),
    "proposal.check" => call(params, |p| service.check(proposal_request(p)?)),
    "proposal.apply" => call(params, |p| service.apply(proposal_request(p)?)),
    "proposal.decode" => call(params, decode_proposal),
    "proposal.encode" => call(params, |p: EncodeRequest<Box<RawValue>>| {
        encode_proposal(EncodeRequest {
            proposal: embedded(&p.proposal)?,
            format: p.format,
        })
    }),
    "analyzers.add" => call(params, |p| service.add_analyzer(p)),
    _ => Err(DispatchError::MethodNotFound),
    }
}

//A A validated single call, ready for dispatch. Parameters stay raw JSON text
//A until the method's own types decode them.
#[derive(Clone, Debug)]
pub struct Request {
    pub id: Value,
    pub method: String,
    params: Option<Box<RawValue>>,
}

impl Request {
    pub fn respond(self, service: &Service) -> Response {
        let params = self.params.as_deref().map_or("{}", RawValue::get);
        let outcome = match dispatch(service, &self.method, params) {
        Ok(result) => Outcome::Result(result),
        Err(DispatchError::MethodNotFound) => Outcome::Error(protocol_error(
            -32601,
            format!("method {:?} not found", self.method),
        )),
        Err(DispatchError::Application(error)) => Outcome::Error(error.to_object()),
        };
        Response::new(self.id, outcome)
    }

    //A Answers the call with an error raised before a service was available,
    //A such as an unusable state path.
    pub fn fail(self, error: &AppError) -> Response {
        Response::new(self.id, Outcome::Error(error.to_object()))
    }
}

//A Request members in source order, so a repeated member is refused rather
//A than silently replaced by the last one.
struct Members(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Members {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MembersVisitor;
        impl<'de> Visitor<'de> for MembersVisitor {
            type Value = Members;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Members, A::Error> {
                let mut members = vec![];
                while let Some(member) = map.next_entry()? {
                    members.push(member);
                }
                Ok(Members(members))
            }
        }
        deserializer.deserialize_map(MembersVisitor)
    }
}

pub fn parse_error(message: impl Into<String>) -> Response {
    Response::new(Value::Null, Outcome::Error(protocol_error(-32700, message)))
}

//A Validate the envelope before dispatch, echoing a usable ID in protocol
//A errors where the request supplied one. HTTP bodies may span lines; the CLI
//A passes exactly one line.
pub fn parse_line(text: &str) -> Result<Request, Response> {
    let invalid = |id: Value, message: String| {
        Response::new(id, Outcome::Error(protocol_error(-32600, message)))
    };
    let request = serde_json::from_str::<&RawValue>(text)
        .map_err(|error| parse_error(format!("parse error: {error}")))?;
    let listed = match request.get().as_bytes().first() {
    Some(b'{') => {
        serde_json::from_str::<Members>(request.get())
            .map_err(|error| invalid(Value::Null, error.to_string()))?
            .0
    }
    Some(b'[') => return Err(invalid(Value::Null, "batches are not supported".into())),
    _ => {
        return Err(invalid(
            Value::Null,
            "the request must be a JSON object".into(),
        ));
    }
    };
    let mut members = BTreeMap::new();
    for (name, value) in listed {
        if members.contains_key(&name) {
            return Err(invalid(
                Value::Null,
                format!("duplicate request member {name:?}"),
            ));
        }
        members.insert(name, value);
    }
    let mut member = |name: &str| {
        members
            .remove(name)
            .and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
    };
    let id = match member("id") {
    Some(id @ (Value::String(_) | Value::Number(_))) => id,
    None | Some(Value::Null) => {
        return Err(invalid(
            Value::Null,
            "notifications are not supported; supply an id".into(),
        ));
    }
    Some(_) => {
        return Err(invalid(
            Value::Null,
            "the id must be a string or number".into(),
        ));
    }
    };
    if member("jsonrpc") != Some(Value::String("2.0".into())) {
        return Err(invalid(id, "jsonrpc must be \"2.0\"".into()));
    }
    let Some(Value::String(method)) = member("method") else {
        return Err(invalid(id, "method must be a string".into()));
    };
    let params = match members.remove("params") {
    None => None,
    Some(params) if params.get().starts_with('{') => Some(params),
    Some(_) => return Err(invalid(id, "params must be an object".into())),
    };
    if let Some(name) = members.keys().next() {
        return Err(invalid(id, format!("unknown request member {name:?}")));
    }
    Ok(Request { id, method, params })
}

pub fn handle_line(service: &Service, line: &str) -> Response {
    match parse_line(line) {
    Ok(request) => request.respond(service),
    Err(response) => response,
    }
}
