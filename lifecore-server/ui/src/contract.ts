// The shared RPC contract, written by hand to match the Rust Serde shapes.
// Rust-emitted fixtures under ./fixtures check it: each one `satisfies` the type
// declared here, so a changed Rust shape fails `tsc`. INTERFACES §3 and §5 own
// the semantics; FILES §2 owns the value encoding.

// Lowercase hyphenated UUID.
export type Uuid = string;
// Canonical decimal integer: `0`, or an optional `-` and digits without a
// leading zero. Integer payloads and large control integers (positions,
// boundaries) use it; keep these strings in model state and use `bigint` only
// for arithmetic.
export type DecimalString = string;
// Jiff's Serde timestamp spelling, kept at full (nanosecond) precision.
export type TimestampString = string;
// `lc1:<database>:<graph revision>:<proposal>`; carried verbatim.
export type PlanId = string;
// Owned by the analyzer that emitted it; never parsed or compared by prefix.
export type AnalyzerCertificate = string;

// ---- Values (FILES §2.1) ----

export type Payload =
  | string
  | Payload[]
  | IntegerPayload
  | BooleanPayload
  | TimestampPayload
  | RecordPayload;
export interface IntegerPayload {
  kind: "integer";
  value: DecimalString;
}
export interface BooleanPayload {
  kind: "boolean";
  value: boolean;
}
export interface TimestampPayload {
  kind: "timestamp";
  value: TimestampString;
}
export interface RecordPayload {
  kind: "record";
  value: Body;
}
export type Body = { [field: string]: Payload };

// ---- Entries and views ----

export interface EntryId {
  origin: Uuid;
  position: DecimalString;
}
export interface ViewId {
  database: Uuid;
  end_exclusive: DecimalString;
  head: EntryId | null;
}
export interface NodeEntry {
  kind: "node";
  id: EntryId;
  type: string;
  // Absent for a root.
  predecessor?: EntryId;
  payload: Body;
}
export interface EdgeEntry {
  kind: "edge";
  id: EntryId;
  type: string;
  from: EntryId;
  to: EntryId;
}
export type Entry = NodeEntry | EdgeEntry;
// A stored entry and whether it is current in the selected view.
export type EntryRecord = (NodeEntry | EdgeEntry) & { current: boolean };

export type RevisionIntent = "revise" | "branch";
export type EntryKind = Entry["kind"];
export type Currentness = "current" | "historical" | "all";

// A core operation, as in suggested actions: a proposal entry without a name,
// whose `added` is the zero-based index of an earlier operation.
export type Reference = { existing: EntryId } | { added: number };
export type EntryChange =
  | {
      kind: "node";
      type: string;
      predecessor?: Reference;
      intent?: RevisionIntent;
      payload: Body;
    }
  | { kind: "edge"; type: string; from: Reference; to: Reference };

// ---- Analysis ----

export interface AnalyzerConfig {
  name: string;
  configuration: string;
}
export interface AnalyzerIdentity {
  name: string;
  version: string;
  configuration: string;
}
export interface EvaluationInputs {
  evaluation_time: TimestampString | null;
}
export type MessageType = "Inconsistency" | "Warning" | "Suggestion";
export interface FindingKey {
  database: Uuid;
  analyzer: AnalyzerIdentity;
  kind: MessageType;
  analyzer_certificate: AnalyzerCertificate;
}
export type MessageKind =
  | "Inconsistency"
  | "Warning"
  | { Suggestion: EntryChange[] };
export interface Diagnostic {
  subjects: EntryId[];
  focus: EntryId | null;
  message: string;
  kind: MessageKind;
  sticky: boolean;
}
// The contiguous range of prefixes, both ends inclusive, at which the same
// diagnostic was emitted.
export type EvaluatedView =
  | "Initialization"
  | { Prefixes: { first: ViewId; last: ViewId } };
export interface Observation {
  key: FindingKey;
  evaluated_view: EvaluatedView;
  diagnostic: Diagnostic;
}
export interface AnalysisOutput {
  view: ViewId;
  inputs: EvaluationInputs;
  implementations: AnalyzerIdentity[];
  observations: Observation[];
}
export interface ExecutionFailure {
  analyzer: AnalyzerIdentity;
  inputs: EvaluationInputs;
  evaluated_view: EvaluatedView;
  message: string;
}

// ---- Proposals (FILES §2) ----

export type ProposalReference = { existing: EntryId } | { added: string };
export type ProposalEntry =
  | {
      name: string;
      kind: "node";
      type: string;
      predecessor?: ProposalReference;
      intent?: RevisionIntent;
      payload: Body;
    }
  | {
      name: string;
      kind: "edge";
      type: string;
      from: ProposalReference;
      to: ProposalReference;
    };
export interface Proposal {
  plan_id?: PlanId;
  acknowledges?: FindingKey;
  entries: ProposalEntry[];
}
export type Format = "json" | "toml";

// ---- Results ----

export interface Description {
  database: Uuid;
  graph_revision: Uuid;
  view: ViewId;
  suite: AnalyzerConfig[];
}
export interface EntryList {
  database: Uuid;
  graph_revision: Uuid;
  view: ViewId;
  entries: EntryRecord[];
}
export interface EntryLookup {
  view: ViewId;
  entry: EntryRecord;
}
export interface MintedPlanId {
  plan_id: PlanId;
}
export interface OperationEntry {
  name: string;
  entry: EntryId;
}
export interface ProposalReport {
  database: Uuid;
  // Absent only for `init`.
  base_revision?: Uuid;
  base: ViewId;
  operations: OperationEntry[];
  analysis: AnalysisOutput;
}
export interface CheckResult {
  accepted: boolean;
  report: ProposalReport;
}
export interface ApplyResult {
  graph_revision: Uuid;
  report: ProposalReport;
}
export interface AnalyzerAdded {
  graph_revision: Uuid;
  suite: AnalyzerConfig[];
  report: ProposalReport;
}
export interface Initialized {
  path: string;
  database: Uuid;
  graph_revision: Uuid;
  suite: AnalyzerConfig[];
}
export interface Encoded {
  text: string;
}
// What a mutation would have returned; carried by `publication_uncertain`.
export type Published = Initialized | ApplyResult | AnalyzerAdded;

// ---- Requests (INTERFACES §3.1) ----

// Parameters never accept null: omit an optional one instead.
export type NoParams = { [name: string]: never };
// An explicit instant or `now`, not both; absent by default.
export interface EvaluationParams {
  evaluation_time?: TimestampString;
  now?: boolean;
}
export interface EntryListParams {
  kind?: EntryKind;
  type?: string;
  currentness?: Currentness;
  text?: string;
}
export interface EntryGetParams {
  id: EntryId;
}
export interface ProposalParams extends EvaluationParams {
  proposal: Proposal;
}
export interface DecodeParams {
  text: string;
  format: Format;
}
export interface EncodeParams {
  proposal: Proposal;
  format: Format;
}
export interface AddAnalyzerParams extends EvaluationParams {
  name: string;
  configuration?: string;
}

export interface Methods {
  "database.describe": { params: NoParams; result: Description };
  "entries.list": { params: EntryListParams; result: EntryList };
  "entries.get": { params: EntryGetParams; result: EntryLookup };
  "plan_id.mint": { params: NoParams; result: MintedPlanId };
  "proposal.check": { params: ProposalParams; result: CheckResult };
  "proposal.apply": { params: ProposalParams; result: ApplyResult };
  "proposal.decode": { params: DecodeParams; result: Proposal };
  "proposal.encode": { params: EncodeParams; result: Encoded };
  "analyzers.add": { params: AddAnalyzerParams; result: AnalyzerAdded };
}
export type Method = keyof Methods;
export type Params<M extends Method> = Methods[M]["params"];
export type Result<M extends Method> = Methods[M]["result"];

// ---- Errors (INTERFACES §3.1) ----

export interface DecodeError {
  format: Format;
  message: string;
  path?: string;
  line?: number;
  column?: number;
}
export interface ProposalLocation {
  operation: number;
  name: string;
  field?: string;
}

// `data` of an application error, by its stable `kind`.
export type ApplicationErrorData =
  | { kind: "state_path_empty" }
  | { kind: "working_directory" }
  | { kind: "directory_missing"; path: string }
  | { kind: "database_missing"; path: string }
  | { kind: "database_unreadable"; path: string }
  | { kind: "database_exists"; path: string }
  | { kind: "lock_failed"; path: string }
  | { kind: "busy"; lock: string }
  | { kind: "publication_failed" }
  | { kind: "publication_uncertain"; outcome: Published }
  | { kind: "malformed_proposal"; decode: DecodeError }
  | { kind: "invalid_proposal"; location?: ProposalLocation }
  | { kind: "acknowledgment_unsupported" }
  | { kind: "foreign_database"; expected: Uuid; actual: Uuid }
  | { kind: "stale_base"; expected: Uuid; actual: Uuid }
  | { kind: "rejected"; report: ProposalReport }
  | { kind: "analyzer_failure"; failure?: ExecutionFailure }
  | { kind: "entry_not_found"; id: EntryId }
  | { kind: "invalid_params" }
  | { kind: "backend_failure" }
  | { kind: "internal" };
export type ErrorKind = ApplicationErrorData["kind"];

export interface ErrorCodes {
  state_path_empty: 1001;
  working_directory: 1001;
  directory_missing: 1001;
  database_missing: 1002;
  database_unreadable: 1003;
  database_exists: 1004;
  lock_failed: 1005;
  busy: 1006;
  publication_failed: 1007;
  publication_uncertain: 1008;
  malformed_proposal: 2001;
  invalid_proposal: 2002;
  acknowledgment_unsupported: 2003;
  foreign_database: 2004;
  stale_base: 2005;
  rejected: 2006;
  analyzer_failure: 3001;
  entry_not_found: 4001;
  invalid_params: -32602;
  backend_failure: 5001;
  internal: -32603;
}

export type ApplicationError<K extends ErrorKind = ErrorKind> =
  K extends ErrorKind
    ? {
        code: ErrorCodes[K];
        message: string;
        data: Extract<ApplicationErrorData, { kind: K }>;
      }
    : never;
// Envelope failures detected before dispatch carry no data.
export interface ProtocolError {
  code: -32700 | -32600 | -32601;
  message: string;
}
export type ErrorObject = ApplicationError | ProtocolError;

// ---- Envelope (INTERFACES §3) ----

export type RequestId = string | number;
export interface RpcRequest<M extends Method = Method> {
  jsonrpc: "2.0";
  id: RequestId;
  method: M;
  params?: Params<M>;
}
export interface RpcSuccess<M extends Method = Method> {
  jsonrpc: "2.0";
  id: RequestId;
  result: Result<M>;
}
// The ID is null when the request had none usable.
export interface RpcFailure<E extends ErrorObject = ErrorObject> {
  jsonrpc: "2.0";
  id: RequestId | null;
  error: E;
}
export type RpcResponse<M extends Method = Method> = RpcSuccess<M> | RpcFailure;

// ---- HTTP access (INTERFACES §3.2) ----

// The JSON body of a request the server refused before dispatch.
export type HttpRejectionKind =
  | "host"
  | "origin"
  | "token"
  | "content_type"
  | "body";
export interface HttpRejection {
  kind: HttpRejectionKind;
  message: string;
}

// ---- Fixture shapes ----

export interface MethodFixture<M extends Method> {
  request: RpcRequest<M>;
  response: RpcSuccess<M>;
}
export type ApplicationErrorFixtures = {
  [K in ErrorKind]: RpcFailure<ApplicationError<K>>[];
};
export type HttpRejectionFixtures = {
  [K in HttpRejectionKind]: {
    status: number;
    body: HttpRejection & { kind: K };
  };
};
