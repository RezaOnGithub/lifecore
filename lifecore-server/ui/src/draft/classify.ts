import type {
  ErrorKind,
  ProposalReport,
  Published,
  Uuid,
} from "../contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import type { Problem } from "./model.ts";
import { pathOperation } from "./operations.ts";

export function message(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

export type Classified =
  | { kind: "stale"; expected: Uuid; actual: Uuid }
  | { kind: "rejected"; report: ProposalReport }
  | {
      kind: "uncertain";
      error: string;
      message: string;
      outcome: Published | null;
    }
  | { kind: "problem"; problem: Problem };

// The error kinds an apply reports before it publishes anything. Every other
// failure of an apply, unknown kinds included, may follow a publication.
const BEFORE_PUBLICATION: ReadonlySet<ErrorKind> = new Set<ErrorKind>([
  "invalid_params",
  "local_time_unknown",
  "malformed_proposal",
  "invalid_proposal",
  "acknowledgment_unsupported",
  "foreign_database",
  "stale_base",
  "rejected",
  "analyzer_failure",
  "busy",
  "lock_failed",
  "database_missing",
  "database_unreadable",
  "publication_failed",
]);

// Sorts a failed check or apply by what the user can do about it. A check
// writes nothing, so a lost answer is only retryable; an apply is uncertain
// unless the failure is known to come before publication, and is never
// retried on its own. `names` are the sent operations, to place a decode
// path.
export function classify(
  failure: unknown,
  action: "check" | "apply",
  names: readonly string[] = [],
): Classified {
  const lost = (error: string, text: string): Classified =>
    action === "apply"
      ? { kind: "uncertain", error, message: text, outcome: null }
      : { kind: "problem", problem: { kind: "retryable", message: text } };
  const failed = (text: string): Classified => ({
    kind: "problem",
    problem: { kind: "failed", message: text, action },
  });
  if (failure instanceof RpcError) {
    const { error } = failure;
    // An envelope error is detected before dispatch.
    if (!("data" in error)) {
      return failed(error.message);
    }
    const { data } = error;
    if (action === "apply" && !BEFORE_PUBLICATION.has(data.kind)) {
      return {
        kind: "uncertain",
        error: data.kind,
        message: error.message,
        outcome: data.kind === "publication_uncertain" ? data.outcome : null,
      };
    }
    switch (data.kind) {
      case "stale_base":
        return { kind: "stale", expected: data.expected, actual: data.actual };
      case "rejected":
        return { kind: "rejected", report: data.report };
      case "invalid_proposal":
        return {
          kind: "problem",
          problem: {
            kind: "invalid_proposal",
            message: error.message,
            location: data.location ?? null,
          },
        };
      case "malformed_proposal":
        return {
          kind: "problem",
          problem: {
            kind: "malformed_proposal",
            message: error.message,
            decode: data.decode,
            operation: pathOperation(data.decode.path, names),
          },
        };
      case "foreign_database":
        return {
          kind: "problem",
          problem: {
            kind: "foreign_database",
            message: error.message,
            expected: data.expected,
            actual: data.actual,
          },
        };
      case "busy":
        return {
          kind: "problem",
          problem: { kind: "busy", message: error.message, action },
        };
      case "backend_failure":
      case "internal":
        return lost(data.kind, error.message);
      default:
        return failed(error.message);
    }
  }
  // A structured rejection comes from the access checks, before dispatch;
  // any other refused answer is not known to.
  if (failure instanceof HttpRejectionError) {
    return failure.rejection === null
      ? lost(`http_${failure.status}`, failure.message)
      : failed(failure.message);
  }
  // A lost answer, or one that could not be read.
  return lost(
    failure instanceof TransportError ? "transport" : "unreadable",
    message(failure),
  );
}
