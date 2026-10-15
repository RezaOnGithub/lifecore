import type { Description, EntryId } from "../contract.ts";
import { IdLink } from "../links.tsx";
import { AddAnalyzer, Verbatim } from "../views/AddAnalyzer.tsx";

function headLabel(id: EntryId | null) {
  return id === null ? "none" : <IdLink id={id} />;
}

export function DatabaseScreen({ description }: { description: Description }) {
  return (
    <section aria-label="Database description">
      <h2>Database</h2>
      <p className="muted">
        The description reads the whole history; the as-of boundary applies to
        the other screens.
      </p>
      <table className="payload">
        <tbody>
          <tr>
            <th scope="row">Database</th>
            <td>
              <code>{description.database}</code>
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
            <td>
              {description.view.end_exclusive}{" "}
              <span className="muted">(whole history)</span>
            </td>
          </tr>
          <tr>
            <th scope="row">Newest entry</th>
            <td>
              {headLabel(description.view.head)}{" "}
              <span className="muted">(whole history)</span>
            </td>
          </tr>
        </tbody>
      </table>
      <h3>Analyzer suite</h3>
      <p className="muted">
        Checks, applies, and analysis runs replay the history under these
        analyzers, in this order. The suite only grows: there is no removal or
        reconfiguration.
      </p>
      {description.suite.length === 0 ? (
        <p data-testid="suite">
          No analyzers: only structural validation guards changes.
        </p>
      ) : (
        <table className="suite" data-testid="suite">
          <thead>
            <tr>
              <th scope="col">#</th>
              <th scope="col">analyzer</th>
              <th scope="col">configuration</th>
            </tr>
          </thead>
          <tbody>
            {description.suite.map((analyzer, index) => (
              // The suite is ordered and may repeat an analyzer.
              // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
              <tr key={index}>
                <td>{index + 1}</td>
                <td>
                  {analyzer.name}
                  {!description.registered.includes(analyzer.name) && (
                    <span
                      className="badge unregistered"
                      title="checks and analysis runs fail until a host that registers it serves the database"
                    >
                      not registered here
                    </span>
                  )}
                </td>
                <td>
                  <Verbatim text={analyzer.configuration} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <AddAnalyzer description={description} />
    </section>
  );
}
