import { HttpRejectionError, RpcError, TransportError } from "./rpc.ts";

export function FailureView({ failure }: { failure: unknown }) {
  if (failure instanceof HttpRejectionError) {
    const token = failure.rejection?.kind === "token";
    return (
      <section className="error" role="alert">
        <h2>
          {token ? "Not authorized" : "Request refused"} (HTTP {failure.status}
          {failure.rejection && `, ${failure.rejection.kind}`})
        </h2>
        <p>{failure.message}</p>
        {token && (
          <p>
            Open the URL that <code>lifecore serve</code> printed. A restarted
            server prints a new URL, and earlier pages must be reopened from it.
          </p>
        )}
      </section>
    );
  }
  if (failure instanceof RpcError) {
    const { error } = failure;
    const data = "data" in error ? error.data : undefined;
    return (
      <section className="error" role="alert">
        <h2>
          {failure.method} failed: error {error.code}
          {data && ` (${data.kind})`}
        </h2>
        <p>{error.message}</p>
        {data && <pre>{JSON.stringify(data, null, 2)}</pre>}
      </section>
    );
  }
  const message =
    failure instanceof TransportError || failure instanceof Error
      ? failure.message
      : String(failure);
  return (
    <section className="error" role="alert">
      <h2>No answer from the server</h2>
      <p>{message}</p>
    </section>
  );
}
