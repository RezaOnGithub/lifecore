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
  ty_name: string;
  // Absent for a root.
  predecessor?: EntryId;
  payload: Body;
}
export interface EdgeEntry {
  kind: "edge";
  id: EntryId;
  ty_name: string;
  from: EntryId;
  to: EntryId;
}
export type Entry = NodeEntry | EdgeEntry;
// A stored entry and whether it is current in the selected view.
export type NodeRecord = NodeEntry & { current: boolean };
export type EdgeRecord = EdgeEntry & { current: boolean };
export type EntryRecord = NodeRecord | EdgeRecord;

export type RevisionIntent = "revise" | "branch";
export type EntryKind = Entry["kind"];
export type Currentness = "current" | "historical" | "all";

// A core operation, as in suggested actions: a proposal entry without a name,
// whose `added` is the zero-based index of an earlier operation.
export type Reference = { existing: EntryId } | { added: number };
export type EntryChange =
  | {
      kind: "node";
      ty_name: string;
      predecessor?: Reference;
      intent?: RevisionIntent;
      payload: Body;
    }
  | { kind: "edge"; ty_name: string; from: Reference; to: Reference };

// ---- Type declarations (FILES §2.6), in structured form ----

export type FieldStatus =
  | "mandatory"
  | "encouraged"
  | "deprecated"
  | "optional";
export type Shape =
  | { kind: "text" }
  | { kind: "integer" }
  | { kind: "boolean" }
  | { kind: "timestamp" }
  | { kind: "list"; element: Shape }
  | { kind: "record"; fields: FieldDecls };
export interface FieldDecl {
  status: FieldStatus;
  shape: Shape;
}
export type FieldDecls = { [field: string]: FieldDecl };
// A parsed declaration: the same information as its word-list payload.
export interface TypeSchema {
  name: string;
  fields: FieldDecls;
  outgoing: string[];
  incoming: string[];
  successors: string[];
}
export type PayloadKind = Shape["kind"];
// What is wrong with a declaration payload at one element.
export type SchemaErrorKind =
  | { kind: "missing_field" }
  | { kind: "unknown_field" }
  | { kind: "wrong_shape"; expected: PayloadKind; actual: PayloadKind }
  | { kind: "missing_presence" }
  | { kind: "unknown_presence"; word: string }
  | { kind: "missing_shape" }
  | { kind: "unknown_shape"; word: string }
  | { kind: "missing_record_fields" }
  | { kind: "trailing_elements" }
  | { kind: "too_deep" }
  | { kind: "duplicate_permission"; name: string };
// Why a declaration payload does not parse: the path of the offending
// element, by field name and list index, beside its kind.
export type SchemaError = { path: (string | number)[] } & SchemaErrorKind;
// A current declaration and how its payload parses.
export type Declaration =
  | { kind: "parsed"; declaration: EntryId; schema: TypeSchema }
  | { kind: "malformed"; declaration: EntryId; error: SchemaError };
// How a type name resolves in the selected view (SPEC §6.2): by the current
// declarations that claim it; `claimants` are in append order.
export type TypeResolution =
  | { kind: "builtin" }
  | { kind: "declared"; declaration: EntryId; schema: TypeSchema }
  | { kind: "missing" }
  | { kind: "ambiguous"; claimants: Declaration[] }
  | { kind: "malformed"; declaration: EntryId; error: SchemaError };
// One type name known to the view: declared, built in, or merely used by a
// node. Counts are of nodes whose `ty_name` is this name.
export interface TypeSummary {
  name: string;
  resolution: TypeResolution;
  nodes: { current: DecimalString; historical: DecimalString };
}
export interface TypeList {
  view: ViewId;
  types: TypeSummary[];
}

// ---- Inspection and queries (SPEC §9) ----

export type Direction = "outgoing" | "incoming" | "both";
// An edge incident to the inspected node, with its other endpoint's id; the
// inspection's `neighbours` hold that endpoint's record.
export interface IncidentEdge {
  edge: EdgeRecord;
  direction: "outgoing" | "incoming";
  other: EntryId;
}
// A node with its type, its whole revision tree in append order (root first,
// itself included), every incident edge in append order, and each other
// endpoint of those edges once, in append order; or an edge with its
// endpoints.
export type EntryInspection =
  | {
      kind: "node";
      view: ViewId;
      entry: NodeRecord;
      type: TypeResolution;
      lineage: NodeRecord[];
      edges: IncidentEdge[];
      neighbours: NodeRecord[];
    }
  | {
      kind: "edge";
      view: ViewId;
      entry: EdgeRecord;
      from: NodeRecord;
      to: NodeRecord;
    };

// A view query: seeds, then expansion steps in order, each following matching
// edges `hops` times (default 1) from the nodes the previous step added, then
// the edges among the result nodes that match `edges` (default: current).
export type Seed =
  | { kind: "ids"; ids: EntryId[] }
  | {
      kind: "nodes";
      ty_name?: string;
      currentness?: Currentness;
      text?: string;
    };
export interface Expansion {
  hops?: number;
  direction?: Direction;
  ty_names?: string[];
  currentness?: Currentness;
}
export interface EdgeFilter {
  ty_names?: string[];
  currentness?: Currentness;
}
// Why a node is in the result: a seed, or first reached at `hop` over edge
// `via` from node `from`.
export type Provenance =
  | { kind: "seed" }
  | { kind: "reached"; hop: number; via: EntryId; from: EntryId };
export interface QueriedNode {
  node: NodeRecord;
  provenance: Provenance;
}
export interface ViewQuery {
  view: ViewId;
  nodes: QueriedNode[];
  edges: EdgeRecord[];
}

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
// A finding's category; a diagnostic carries the same word in its `kind`.
export type MessageType = "inconsistency" | "warning" | "suggestion";
export interface FindingKey {
  database: Uuid;
  analyzer: AnalyzerIdentity;
  kind: MessageType;
  analyzer_certificate: AnalyzerCertificate;
}
interface DiagnosticBody {
  subjects: EntryId[];
  focus: EntryId | null;
  message: string;
  sticky: boolean;
}
// A suggestion's ordered actions, possibly empty, sit beside its `kind`.
export type Diagnostic =
  | (DiagnosticBody & { kind: "inconsistency" | "warning" })
  | (DiagnosticBody & { kind: "suggestion"; actions: EntryChange[] });
// The contiguous range of prefixes, both ends inclusive, at which the same
// diagnostic was emitted.
export type EvaluatedView =
  | { kind: "initialization" }
  | { kind: "prefixes"; first: ViewId; last: ViewId };
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
// A stored dismissal record (INTERFACES §6), keyed by the complete finding
// key; an inconsistency can never have one.
export type DismissalReason =
  | "acknowledged"
  | "false_positive"
  | "no_longer_relevant";
export interface Dismissal {
  key: FindingKey;
  reason: DismissalReason;
  note: string | null;
}

// ---- Proposals (FILES §2) ----

export type ProposalReference = { existing: EntryId } | { added: string };
export type ProposalEntry =
  | {
      name: string;
      kind: "node";
      ty_name: string;
      predecessor?: ProposalReference;
      intent?: RevisionIntent;
      payload: Body;
    }
  | {
      name: string;
      kind: "edge";
      ty_name: string;
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
  // The analyzer names this host can add, sorted.
  registered: string[];
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
// A replayed analysis beside the stored dismissal records, from the same
// snapshot read, of the findings it emitted; presentation applies them, the
// analysis never does.
export interface AnalysisRun {
  analysis: AnalysisOutput;
  dismissals: Dismissal[];
}
// Every stored dismissal record after an edit, in storage order.
export interface DismissalList {
  dismissals: Dismissal[];
}
export interface RestoreResult {
  dismissals: Dismissal[];
  restored: boolean;
}
export type RenderFormat = "dot" | "svg";
export interface Rendered {
  format: RenderFormat;
  text: string;
}
// What a mutation would have returned, tagged by `kind`; carried by
// `publication_uncertain`. Both dismissal edits carry the stored records.
export type Published =
  | ({ kind: "initialized" } & Initialized)
  | ({ kind: "applied" } & ApplyResult)
  | ({ kind: "analyzer_added" } & AnalyzerAdded)
  | ({ kind: "dismissed" } & DismissalList)
  | ({ kind: "restored" } & RestoreResult);

// ---- Requests (INTERFACES §3.1) ----

// Parameters never accept null: omit an optional one instead.
export type NoParams = { [name: string]: never };
// An explicit instant or `now`, not both; absent by default.
export interface EvaluationParams {
  evaluation_time?: TimestampString;
  now?: boolean;
}
// Read methods answer as of the prefix `[0, as_of)`; default: the whole
// history. A boundary past the end fails with `beyond_boundary`.
export interface AsOfParams {
  as_of?: DecimalString;
}
export interface EntryListParams extends AsOfParams {
  kind?: EntryKind;
  ty_name?: string;
  currentness?: Currentness;
  text?: string;
}
export interface EntryGetParams extends AsOfParams {
  id: EntryId;
}
export interface EntryInspectParams extends AsOfParams {
  id: EntryId;
}
export interface ViewQueryParams extends AsOfParams {
  seeds: Seed[];
  expand?: Expansion[];
  edges?: EdgeFilter;
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
export interface AnalysisRunParams extends AsOfParams, EvaluationParams {}
export interface DismissParams {
  key: FindingKey;
  reason: DismissalReason;
  note?: string;
}
export interface RestoreParams {
  key: FindingKey;
}
export interface ViewRenderParams {
  query: ViewQueryParams;
  format: RenderFormat;
}

export interface Methods {
  "database.describe": { params: NoParams; result: Description };
  "entries.list": { params: EntryListParams; result: EntryList };
  "entries.get": { params: EntryGetParams; result: EntryLookup };
  "entries.inspect": { params: EntryInspectParams; result: EntryInspection };
  "types.list": { params: AsOfParams; result: TypeList };
  "views.query": { params: ViewQueryParams; result: ViewQuery };
  "views.render": { params: ViewRenderParams; result: Rendered };
  "plan_id.mint": { params: NoParams; result: MintedPlanId };
  "proposal.check": { params: ProposalParams; result: CheckResult };
  "proposal.apply": { params: ProposalParams; result: ApplyResult };
  "proposal.decode": { params: DecodeParams; result: Proposal };
  "proposal.encode": { params: EncodeParams; result: Encoded };
  "analyzers.add": { params: AddAnalyzerParams; result: AnalyzerAdded };
  "analysis.run": { params: AnalysisRunParams; result: AnalysisRun };
  "findings.dismiss": { params: DismissParams; result: DismissalList };
  "findings.restore": { params: RestoreParams; result: RestoreResult };
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
// An `ids` seed that names no node of the view: absent from it, or an edge.
export interface RefusedSeed {
  id: EntryId;
  reason: "absent" | "edge";
}
// Why `dot` failed after it started. Durations are in milliseconds.
export type GraphvizFailure =
  | { kind: "pipe_thread"; message: string }
  | { kind: "unwaitable"; message: string }
  | { kind: "deadline"; milliseconds: number }
  | { kind: "oversized"; bytes: number }
  | { kind: "exited"; status: string }
  | { kind: "output_unreadable"; message: string }
  | { kind: "output_open"; milliseconds: number }
  | { kind: "input_unread"; message: string | null }
  | { kind: "stderr_unreadable"; message: string }
  | { kind: "invalid_utf8"; message: string };

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
  | {
      kind: "beyond_boundary";
      requested: DecimalString;
      visible: DecimalString;
    }
  | { kind: "cannot_dismiss_inconsistency"; key: FindingKey }
  | { kind: "graphviz_unavailable"; message: string }
  | { kind: "graphviz_failed"; cause: GraphvizFailure; stderr: string }
  | { kind: "invalid_seeds"; refused: RefusedSeed[] }
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
  beyond_boundary: 4002;
  cannot_dismiss_inconsistency: 4003;
  graphviz_unavailable: 4004;
  graphviz_failed: 4005;
  invalid_seeds: 4006;
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
// The HTTP status that accompanies each rejection kind: the body check answers
// 413 for the size limit and 400 for a body that cannot be read.
export interface HttpRejectionStatuses {
  host: 421;
  origin: 403;
  token: 401;
  content_type: 415;
  body: 413 | 400;
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
    status: HttpRejectionStatuses[K];
    body: HttpRejection & { kind: K };
  }[];
};
