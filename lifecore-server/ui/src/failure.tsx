import type { ReactNode } from "react";
import { HttpRejectionError, RpcError, TransportError } from "./rpc.ts";

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
    return {
      heading: (
        <>
          {failure.method} failed: error {error.code}
          {data && ` (${data.kind})`}
        </>
      ),
      message: error.message,
      details: data && <pre>{JSON.stringify(data, null, 2)}</pre>,
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
