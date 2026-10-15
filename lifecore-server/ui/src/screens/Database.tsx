import type { Description, EntryId } from "../contract.ts";
import { IdLink } from "../links.tsx";

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
