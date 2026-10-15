import type { Body, FieldStatus, Payload, TypeSchema } from "../contract.ts";
import { fieldColumns } from "../model/columns.ts";
import { formatPayload, shapeWords, truncate } from "../model/payload.ts";

const CELL_LIMIT = 80;

export function ValueCell({ value }: { value: Payload | undefined }) {
  if (value === undefined) {
    return <td className="cell absent" />;
  }
  const text = formatPayload(value);
  return (
    <td className="cell" title={text.length > CELL_LIMIT ? text : undefined}>
      {truncate(text, CELL_LIMIT)}
    </td>
  );
}

export function StatusBadge({ status }: { status: FieldStatus }) {
  return <span className={`badge status-${status}`}>{status}</span>;
}

// A payload as field rows. With a schema every declared field appears, present
// or absent, and undeclared present fields are flagged; without one the
// present fields are listed as they are.
export function PayloadTable({
  payload,
  schema,
}: {
  payload: Body;
  schema: TypeSchema | null;
}) {
  const columns = fieldColumns(schema, [{ payload }]);
  if (columns.length === 0) {
    return <p className="muted">empty payload</p>;
  }
  return (
    <table className="payload">
      <thead>
        <tr>
          <th scope="col">field</th>
          {schema && <th scope="col">status</th>}
          {schema && <th scope="col">shape</th>}
          <th scope="col">value</th>
        </tr>
      </thead>
      <tbody>
        {columns.map((column) => {
          const value = payload[column.field];
          const decl = schema?.fields[column.field];
          return (
            <tr key={column.field}>
              <th scope="row">{column.field}</th>
              {schema && (
                <td>
                  {decl ? (
                    <StatusBadge status={decl.status} />
                  ) : (
                    <span className="badge warn">undeclared</span>
                  )}
                </td>
              )}
              {schema && (
                <td className="muted">{decl ? shapeWords(decl.shape) : ""}</td>
              )}
              {value === undefined ? (
                <td className="muted">absent</td>
              ) : (
                <ValueCell value={value} />
              )}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export function RawJson({ value }: { value: unknown }) {
  return <pre className="raw">{JSON.stringify(value, null, 2)}</pre>;
}

function names(list: string[]): string {
  return list.length === 0 ? "none" : list.join(", ");
}

// A parsed type declaration: its fields in status order and its permissions.
export function SchemaView({ schema }: { schema: TypeSchema }) {
  const columns = fieldColumns(schema, []);
  return (
    <>
      {columns.length === 0 ? (
        <p className="muted">no fields</p>
      ) : (
        <table className="payload">
          <thead>
            <tr>
              <th scope="col">field</th>
              <th scope="col">status</th>
              <th scope="col">shape</th>
            </tr>
          </thead>
          <tbody>
            {columns.map((column) => {
              const decl = schema.fields[column.field];
              return (
                <tr key={column.field}>
                  <th scope="row">{column.field}</th>
                  <td>{decl && <StatusBadge status={decl.status} />}</td>
                  <td>{decl && shapeWords(decl.shape)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <dl className="permissions">
        <dt>outgoing</dt>
        <dd>{names(schema.outgoing)}</dd>
        <dt>incoming</dt>
        <dd>{names(schema.incoming)}</dd>
        <dt>successors</dt>
        <dd>{names(schema.successors)}</dd>
      </dl>
    </>
  );
}
