import { memo, type ReactNode, useCallback, useMemo } from "react";
import type { NodeRecord, Payload, TypeSchema } from "../contract.ts";
import { CurrentBadge, NodeLink, Position } from "../links.tsx";
import {
  type FieldColumn,
  fieldColumns,
  sortedBy,
  toggleSort,
} from "../model/columns.ts";
import { commonLabelField, entryKey, rowLabel } from "../model/labels.ts";
import { declarationSummary } from "../model/schema.ts";
import { ROW_PAGE, type TableView } from "../navigation.ts";
import { ValueCell } from "./Payload.tsx";

export interface ExtraColumn<R extends NodeRecord> {
  key: string;
  header: ReactNode;
  // `open` is the cell's expansion, kept in the table's view.
  cell: (row: R, open: boolean, setOpen: (open: boolean) => void) => ReactNode;
  // Omitted for a column that does not sort.
  sortValue?: (row: R) => Payload | undefined;
  className?: string;
}

type Update = (update: (view: TableView) => TableView) => void;

// An open cell is `<row key>|<column key>`; a row key never holds `|`.
const openKey = (row: string, column: string) => `${row}|${column}`;
// A row's open column keys travel to it as one string, so its memo
// compares them by value.
const JOIN = "\u0000";

// Rows of one type: `lead` columns, the label, `@position`, currentness
// when the rows mix it, `extra` columns (via, relationships), then the
// payload. The built-in `Type` lists declarations with a summary instead of
// a column per declared field; the field that labels every row has no
// column of its own, since the label shows it. At most `view.limit` rows
// render.
function EntryTableOf<R extends NodeRecord>({
  rows,
  schema,
  ty_name,
  lead = NONE,
  extra = NONE,
  showCurrent,
  view,
  onView,
}: {
  rows: readonly R[];
  schema: TypeSchema | null;
  ty_name: string;
  lead?: readonly ExtraColumn<R>[];
  extra?: readonly ExtraColumn<R>[];
  showCurrent: boolean;
  view: TableView;
  onView: Update;
}) {
  const typeTable = ty_name === "Type";
  const fields = useMemo(() => {
    if (typeTable) {
      return [];
    }
    const labelled = commonLabelField(rows);
    return fieldColumns(schema, rows).filter(
      (column) => column.field !== labelled,
    );
  }, [typeTable, schema, rows]);
  const { sort } = view;
  const sorted = useMemo(() => {
    if (sort === null) {
      return rows;
    }
    // Column keys are namespaced so a payload field can never collide with
    // a fixed column.
    const { column } = sort;
    const value = (row: R): Payload | undefined => {
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
      const found = [...lead, ...extra].find(
        (c) => `extra:${c.key}` === column,
      );
      return found?.sortValue?.(row);
    };
    return sortedBy(rows, value, sort.descending);
  }, [rows, sort, lead, extra]);
  const opened = useMemo(() => {
    const byRow = new Map<string, string[]>();
    for (const key of view.open) {
      const split = key.indexOf("|");
      const row = key.slice(0, split);
      byRow.set(row, [...(byRow.get(row) ?? []), key.slice(split + 1)]);
    }
    return byRow;
  }, [view.open]);
  const toggle = useCallback(
    (row: string, column: string, open: boolean) =>
      onView((current) => {
        const key = openKey(row, column);
        const others = current.open.filter((existing) => existing !== key);
        return { ...current, open: open ? [...others, key] : others };
      }),
    [onView],
  );
  if (rows.length === 0) {
    return <p className="muted">no rows</p>;
  }
  const shown =
    view.limit === null ? sorted : sorted.slice(0, Math.max(view.limit, 1));
  const hidden = sorted.length - shown.length;
  // The sorted column says so through `aria-sort`; its arrow is only drawn,
  // so the button keeps the column's name as its accessible name.
  const header = (key: string, label: ReactNode, sortable = true) => {
    const sorted = sortable && sort?.column === key ? sort : null;
    return (
      <th
        key={key}
        scope="col"
        aria-sort={
          sorted === null
            ? undefined
            : sorted.descending
              ? "descending"
              : "ascending"
        }
      >
        {sortable ? (
          <button
            type="button"
            className="sort"
            onClick={() =>
              onView((current) => ({
                ...current,
                sort: toggleSort(current.sort, key),
              }))
            }
          >
            {label}
            {sorted !== null && (
              <span aria-hidden="true">{sorted.descending ? " ▼" : " ▲"}</span>
            )}
          </button>
        ) : (
          label
        )}
      </th>
    );
  };
  return (
    <>
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
            {showCurrent && header("current", "current")}
            {typeTable && header("declaration", "declaration")}
            {extra.map((column) =>
              header(
                `extra:${column.key}`,
                column.header,
                column.sortValue !== undefined,
              ),
            )}
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
          </tr>
        </thead>
        <tbody>
          {shown.map((row) => {
            const key = entryKey(row.id);
            return (
              <Row
                key={key}
                row={row}
                lead={lead}
                extra={extra}
                fields={fields}
                typeTable={typeTable}
                showCurrent={showCurrent}
                open={opened.get(key)?.join(JOIN) ?? ""}
                toggle={toggle}
              />
            );
          })}
        </tbody>
      </table>
      {hidden > 0 && (
        <p className="row more-rows" data-testid="more-rows">
          <span className="muted">
            {shown.length} of {sorted.length} rows shown, {hidden} more
          </span>
          <button
            type="button"
            onClick={() =>
              onView((current) => ({
                ...current,
                limit: (current.limit ?? ROW_PAGE) + ROW_PAGE,
              }))
            }
          >
            show next {Math.min(ROW_PAGE, hidden)}
          </button>
          <button
            type="button"
            onClick={() => onView((current) => ({ ...current, limit: null }))}
          >
            show all
          </button>
        </p>
      )}
    </>
  );
}

const NONE: readonly never[] = [];

// One row, re-rendered only when its own entry, the columns, or its open
// cells change; a sort moves rows without rendering them again.
function RowOf<R extends NodeRecord>({
  row,
  lead,
  extra,
  fields,
  typeTable,
  showCurrent,
  open,
  toggle,
}: {
  row: R;
  lead: readonly ExtraColumn<R>[];
  extra: readonly ExtraColumn<R>[];
  fields: readonly FieldColumn[];
  typeTable: boolean;
  showCurrent: boolean;
  open: string;
  toggle: (row: string, column: string, open: boolean) => void;
}) {
  const key = entryKey(row.id);
  const opened = open === "" ? [] : open.split(JOIN);
  const cell = (column: ExtraColumn<R>) =>
    column.cell(row, opened.includes(column.key), (on) =>
      toggle(key, column.key, on),
    );
  return (
    <tr className={row.current ? undefined : "historical"}>
      {lead.map((column) => (
        <td key={column.key} className={column.className}>
          {cell(column)}
        </td>
      ))}
      <td className="label-cell">
        <NodeLink node={row} position={false} />
      </td>
      <td>
        <Position id={row.id} />
      </td>
      {showCurrent && (
        <td>
          <CurrentBadge current={row.current} kind="node" />
        </td>
      )}
      {typeTable && <td className="cell">{declarationSummary(row.payload)}</td>}
      {extra.map((column) => (
        <td key={column.key} className={column.className}>
          {cell(column)}
        </td>
      ))}
      {fields.map((column) => (
        <ValueCell key={column.field} value={row.payload[column.field]} />
      ))}
    </tr>
  );
}

const Row = memo(RowOf) as typeof RowOf;

export const EntryTable = memo(EntryTableOf) as typeof EntryTableOf;
