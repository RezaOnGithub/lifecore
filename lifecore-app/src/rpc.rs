use crate::{
    AppError, ErrorObject,
    proposal::{self, Format, Proposal},
    service::Service,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

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
pub struct EncodeRequest {
    pub proposal: Proposal,
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
    params: Value,
    operation: impl FnOnce(P) -> Result<R, AppError>,
) -> Result<Value, DispatchError> {
    let params = serde_json::from_value(params).map_err(|error| AppError::InvalidParams {
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

//A Method names are provisional until the RPC contract is frozen (PLAN 4.2).
//A `init` and filesystem path selection are deliberately absent.
fn dispatch(service: &Service, method: &str, params: Value) -> Result<Value, DispatchError> {
    match method {
    "database.describe" => call(params, |p| service.describe(p)),
    "entries.list" => call(params, |p| service.list_entries(p)),
    "entries.get" => call(params, |p| service.get_entry(p)),
    "plan_id.mint" => call(params, |p| service.mint_plan_id(p)),
    "proposal.check" => call(params, |p| service.check(p)),
    "proposal.apply" => call(params, |p| service.apply(p)),
    "proposal.decode" => call(params, decode_proposal),
    "proposal.encode" => call(params, encode_proposal),
    "analyzers.add" => call(params, |p| service.add_analyzer(p)),
    _ => Err(DispatchError::MethodNotFound),
    }
}

//A A validated single call, ready for dispatch.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn respond(self, service: &Service) -> Response {
        let outcome = match dispatch(service, &self.method, self.params) {
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

//A Validate the envelope before dispatch, echoing a usable ID in protocol
//A errors where the request supplied one.
pub fn parse_value(request: Value) -> Result<Request, Response> {
    let invalid = |id: Value, message: &str| {
        Response::new(id, Outcome::Error(protocol_error(-32600, message)))
    };
    let mut request = match request {
    Value::Object(request) => request,
    Value::Array(_) => return Err(invalid(Value::Null, "batches are not supported")),
    _ => return Err(invalid(Value::Null, "the request must be a JSON object")),
    };
    let id = match request.remove("id") {
    Some(id @ (Value::String(_) | Value::Number(_))) => id,
    None | Some(Value::Null) => {
        return Err(invalid(
            Value::Null,
            "notifications are not supported; supply an id",
        ));
    }
    Some(_) => return Err(invalid(Value::Null, "the id must be a string or number")),
    };
    if request.remove("jsonrpc") != Some(Value::String("2.0".into())) {
        return Err(invalid(id, "jsonrpc must be \"2.0\""));
    }
    let Some(Value::String(method)) = request.remove("method") else {
        return Err(invalid(id, "method must be a string"));
    };
    let params = match request.remove("params") {
    None => Value::Object(Map::new()),
    Some(params @ Value::Object(_)) => params,
    Some(_) => return Err(invalid(id, "params must be an object")),
    };
    if let Some(member) = request.keys().next() {
        return Err(invalid(id, &format!("unknown request member {member:?}")));
    }
    Ok(Request { id, method, params })
}

pub fn parse_error(message: impl Into<String>) -> Response {
    Response::new(Value::Null, Outcome::Error(protocol_error(-32700, message)))
}

pub fn parse_line(line: &str) -> Result<Request, Response> {
    let value = serde_json::from_str::<Value>(line)
        .map_err(|error| parse_error(format!("parse error: {error}")))?;
    parse_value(value)
}

pub fn handle_line(service: &Service, line: &str) -> Response {
    match parse_line(line) {
    Ok(request) => request.respond(service),
    Err(response) => response,
    }
}
