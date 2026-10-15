import { type ReactNode, useState } from "react";
import type { NodeRecord, Payload, TypeSchema } from "../contract.ts";
import { CurrentBadge, NodeLink, Position } from "../links.tsx";
import {
  fieldColumns,
  type Sort,
  sortedBy,
  toggleSort,
} from "../model/columns.ts";
import { entryKey, rowLabel } from "../model/labels.ts";
import { declarationSummary } from "../model/schema.ts";
import { ValueCell } from "./Payload.tsx";

export interface ExtraColumn<R extends NodeRecord> {
  key: string;
  header: ReactNode;
  cell: (row: R) => ReactNode;
  // Omitted for a column that does not sort.
  sortValue?: (row: R) => Payload | undefined;
  className?: string;
}

// Rows of one type. The built-in `Type` lists declarations with a summary
// instead of a column per declared field. `lead` columns come before the
// label, `extra` ones after the payload.
export function EntryTable<R extends NodeRecord>({
  rows,
  schema,
  ty_name,
  lead = [],
  extra = [],
}: {
  rows: readonly R[];
  schema: TypeSchema | null;
  ty_name: string;
  lead?: ExtraColumn<R>[];
  extra?: ExtraColumn<R>[];
}) {
  const [sort, setSort] = useState<Sort | null>(null);
  const typeTable = ty_name === "Type";
  const fields = typeTable ? [] : fieldColumns(schema, rows);
  // Column keys are namespaced so a payload field can never collide with a
  // fixed column.
  const sortValue = (row: R, column: string): Payload | undefined => {
    if (column === "label") {
      return rowLabel(row);
    }
    if (column === "position") {
      return { kind: "integer", value: row.id.position };
    }
    if (column === "current") {
      return row.current ? "current" : "superseded";
    }
    if (column === "declaration") {
      return declarationSummary(row.payload);
    }
    if (column.startsWith("field:")) {
      return row.payload[column.slice("field:".length)];
    }
    const column_ = [...lead, ...extra].find(
      (c) => `extra:${c.key}` === column,
    );
    return column_?.sortValue?.(row);
  };
  const sorted =
    sort === null
      ? rows
      : sortedBy(rows, (row) => sortValue(row, sort.column), sort.descending);
  const header = (key: string, label: ReactNode, sortable = true) => (
    <th key={key} scope="col">
      {sortable ? (
        <button
          type="button"
          className="sort"
          onClick={() => setSort((current) => toggleSort(current, key))}
        >
          {label}
          {sort?.column === key && (sort.descending ? " ▼" : " ▲")}
        </button>
      ) : (
        label
      )}
    </th>
  );
  if (rows.length === 0) {
    return <p className="muted">no rows</p>;
  }
  return (
    <table className="entries">
      <thead>
        <tr>
          {lead.map((column) =>
            header(
              `extra:${column.key}`,
              column.header,
              column.sortValue !== undefined,
            ),
          )}
          {header("label", "label")}
          {header("position", "@position")}
          {header("current", "current")}
          {typeTable && header("declaration", "declaration")}
          {fields.map((column) =>
            header(
              `field:${column.field}`,
              <span
                className={
                  column.status ? `col-${column.status}` : "col-undeclared"
                }
                title={column.status ?? "undeclared"}
              >
                {column.field}
              </span>,
            ),
          )}
          {extra.map((column) =>
            header(
              `extra:${column.key}`,
              column.header,
              column.sortValue !== undefined,
            ),
          )}
        </tr>
      </thead>
      <tbody>
        {sorted.map((row) => (
          <tr
            key={entryKey(row.id)}
            className={row.current ? undefined : "historical"}
          >
            {lead.map((column) => (
              <td key={column.key} className={column.className}>
                {column.cell(row)}
              </td>
            ))}
            <td className="label-cell">
              <NodeLink node={row} position={false} />
            </td>
            <td>
              <Position id={row.id} />
            </td>
            <td>
              <CurrentBadge current={row.current} kind="node" />
            </td>
            {typeTable && (
              <td className="cell">{declarationSummary(row.payload)}</td>
            )}
            {fields.map((column) => (
              <ValueCell key={column.field} value={row.payload[column.field]} />
            ))}
            {extra.map((column) => (
              <td key={column.key} className={column.className}>
                {column.cell(row)}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
