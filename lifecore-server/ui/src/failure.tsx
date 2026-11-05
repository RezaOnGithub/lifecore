import type { ReactNode } from "react";
import type { ApplicationErrorData, ErrorKind } from "./contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "./rpc.ts";

// Headings for the kinds a reader is likely to meet. Any other kind,
// including one this client does not know yet, reads by the server's
// message under a generic heading.
const HEADINGS: Partial<Record<ErrorKind, string>> = {
  entry_not_found: "Entry not in this view",
  beyond_boundary: "Boundary past the end",
  database_missing: "No database",
  database_unreadable: "The database cannot be read",
  lock_failed: "The database cannot be locked",
  busy: "The database is busy",
  analyzer_failure: "An analyzer failed",
  graphviz_unavailable: "Graphviz is not available",
  graphviz_failed: "Graphviz could not draw this view",
  invalid_seeds: "Query seeds refused",
  invalid_params: "The server did not accept the request",
  backend_failure: "The server failed while answering",
  internal: "Internal server error",
};

// A sentence in place of a server message that names entries the way the
// screens do not, as `EntryId { position: …, origin: … }`; null keeps the
// server's message.
function sentence(data: ApplicationErrorData): string | null {
  switch (data.kind) {
    case "entry_not_found":
      return `Entry @${data.id.position} is not in this view: the view ends before it, or the database holds another entry at that position.`;
    case "beyond_boundary":
      return `The as-of boundary ${data.requested} is past the end of the history, which has ${data.visible} entries.`;
    case "invalid_seeds":
      return `A query starts only from nodes of this view: ${data.refused
        .map(
          ({ id, reason }) =>
            `@${id.position} ${reason === "absent" ? "is not in this view" : "is an edge"}`,
        )
        .join(", ")}.`;
    default:
      return null;
  }
}

function describe(failure: unknown): {
  heading: ReactNode;
  message: string;
  details: ReactNode;
} {
  if (failure instanceof HttpRejectionError) {
    const token = failure.rejection?.kind === "token";
    return {
      heading: (
        <>
          {token ? "Not authorized" : "Request refused"} (HTTP {failure.status}
          {failure.rejection && `, ${failure.rejection.kind}`})
        </>
      ),
      message: failure.message,
      details: token && (
        <p>
          Open the URL that <code>lifecore serve</code> printed. A restarted
          server prints a new URL, and earlier pages must be reopened from it.
        </p>
      ),
    };
  }
  if (failure instanceof RpcError) {
    const { error } = failure;
    const data = "data" in error ? error.data : undefined;
    const replaced = data === undefined ? null : sentence(data);
    return {
      heading: (data && HEADINGS[data.kind]) ?? "The server could not answer",
      message: replaced ?? error.message,
      details: (
        <details>
          <summary>Details</summary>
          <p>
            <code>{failure.method}</code> answered error {error.code}
            {data && (
              <>
                {" "}
                (<code>{data.kind}</code>)
              </>
            )}
            {replaced !== null && `: ${error.message}`}
          </p>
          {data && <pre>{JSON.stringify(data, null, 2)}</pre>}
        </details>
      ),
    };
  }
  return {
    heading: "No answer from the server",
    message:
      failure instanceof TransportError || failure instanceof Error
        ? failure.message
        : String(failure),
    details: null,
  };
}

// `compact` keeps the heading and message only, for narrow places such as
// the sidebar.
export function FailureView({
  failure,
  compact = false,
}: {
  failure: unknown;
  compact?: boolean;
}) {
  const { heading, message, details } = describe(failure);
  return (
    <section className={compact ? "error compact" : "error"} role="alert">
      <h2>{heading}</h2>
      <p>{message}</p>
      {!compact && details}
    </section>
  );
}
