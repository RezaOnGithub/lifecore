import { useEffect, useState } from "react";
import type { Description, EntryId } from "./contract.ts";
import {
  HttpRejectionError,
  type RpcClient,
  RpcError,
  TransportError,
} from "./rpc.ts";

type Load =
  | { status: "loading" }
  | { status: "ready"; description: Description }
  | { status: "failed"; failure: unknown };

export function App({ client }: { client: RpcClient }) {
  const [load, setLoad] = useState<Load>({ status: "loading" });
  useEffect(() => {
    let active = true;
    client.call("database.describe", {}).then(
      (description) => active && setLoad({ status: "ready", description }),
      (failure: unknown) => active && setLoad({ status: "failed", failure }),
    );
    return () => {
      active = false;
    };
  }, [client]);
  return (
    <main>
      <h1>LifeCore</h1>
      {load.status === "loading" && <p>Loading the database description…</p>}
      {load.status === "ready" && (
        <DescriptionView description={load.description} />
      )}
      {load.status === "failed" && <FailureView failure={load.failure} />}
    </main>
  );
}

function entryLabel(id: EntryId | null): string {
  return id === null ? "none" : `${id.position} @ ${id.origin}`;
}

function DescriptionView({ description }: { description: Description }) {
  return (
    <section aria-label="Database description">
      <table>
        <tbody>
          <tr>
            <th scope="row">Database</th>
            <td>
              <code data-testid="database">{description.database}</code>
            </td>
          </tr>
          <tr>
            <th scope="row">Graph revision</th>
            <td>
              <code>{description.graph_revision}</code>
            </td>
          </tr>
          <tr>
            <th scope="row">Entries</th>
            <td data-testid="entries">{description.view.end_exclusive}</td>
          </tr>
          <tr>
            <th scope="row">Newest entry</th>
            <td>
              <code>{entryLabel(description.view.head)}</code>
            </td>
          </tr>
          <tr>
            <th scope="row">Analyzer suite</th>
            <td>
              {description.suite.length === 0 ? (
                "none"
              ) : (
                <ol>
                  {description.suite.map((analyzer, index) => (
                    // The suite is ordered and may repeat an analyzer.
                    // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
                    <li key={index}>
                      {analyzer.name} <code>{analyzer.configuration}</code>
                    </li>
                  ))}
                </ol>
              )}
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  );
}

function FailureView({ failure }: { failure: unknown }) {
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
