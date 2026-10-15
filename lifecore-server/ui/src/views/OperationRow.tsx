import { useState } from "react";
import type {
  DecimalString,
  ProposalEntry,
  ProposalReference,
} from "../contract.ts";
import { useDraft } from "../draft/context.tsx";
import { countFindings } from "../draft/findings.ts";
import { isUnset, type KeptEdge, type Problem } from "../draft/model.ts";
import {
  endpointWarnings,
  keptWarning,
  referenceIssue,
} from "../draft/operations.ts";
import { IdLink, OperationLink, useOpen } from "../links.tsx";
import { nameError } from "../model/values.ts";
import { CheckStatus, FindingList, type Judged } from "./Findings.tsx";

function Reference({
  reference,
  entries,
  index,
}: {
  reference: ProposalReference;
  entries: readonly ProposalEntry[];
  index: number;
}) {
  if ("existing" in reference) {
    return isUnset(reference) ? (
      <span className="ref-unset" title="not picked yet">
        ?
      </span>
    ) : (
      <IdLink id={reference.existing} />
    );
  }
  const issue = referenceIssue(entries, index, reference);
  return (
    <span
      className={issue === null ? undefined : "ref-issue"}
      title={
        issue === "missing"
          ? "no operation has this name"
          : issue === "later"
            ? "refers to a later operation"
            : undefined
      }
    >
      <OperationLink name={reference.added} />
    </span>
  );
}

function ReferenceChips({
  entry,
  entries,
  index,
}: {
  entry: ProposalEntry;
  entries: readonly ProposalEntry[];
  index: number;
}) {
  if (entry.kind === "node") {
    return entry.predecessor === undefined ? null : (
      <span className="chip">
        {entry.intent ?? "revises?"}{" "}
        <Reference
          reference={entry.predecessor}
          entries={entries}
          index={index}
        />
      </span>
    );
  }
  return (
    <span className="chip">
      <Reference reference={entry.from} entries={entries} index={index} /> →{" "}
      <Reference reference={entry.to} entries={entries} index={index} />
    </span>
  );
}

// The operation a content problem from the server names.
export function flagged(problem: Problem | null, name: string): boolean {
  switch (problem?.kind) {
    case "invalid_proposal":
      return problem.location?.name === name;
    case "malformed_proposal":
    case "incomplete":
      return problem.operation === name;
    default:
      return false;
  }
}

export function operationWarnings(
  entries: readonly ProposalEntry[],
  index: number,
  kept: Record<string, KeptEdge>,
): string[] {
  const entry = entries[index];
  if (entry === undefined) {
    return [];
  }
  const warning = keptWarning(entries, kept[entry.name]);
  return [
    ...endpointWarnings(entries, index),
    ...(warning === null ? [] : [warning]),
  ];
}

interface RowProps {
  entry: ProposalEntry;
  index: number;
  base: DecimalString | null;
  selected: string | null;
  judgment: Judged | null;
}

// One compact line per operation: provisional position, name, kind, type,
// references, and status; warnings and findings follow when there are any.
function OperationRow({ entry, index, base, selected, judgment }: RowProps) {
  const { state, dispatch } = useDraft();
  const open = useOpen();
  const { entries } = state.proposal;
  const warnings = operationWarnings(entries, index, state.kept);
  const problem = nameError(entry.name);
  const findings = judgment?.findings(entry.name);
  const classes = [
    "op",
    entry.name === selected && "selected",
    flagged(state.problem, entry.name) && "invalid",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <li className={classes} data-testid="operation">
      <div className="op-line">
        {base !== null && (
          <code className="pos dim" title="provisional position">
            @{(BigInt(base) + BigInt(index)).toString()}
          </code>
        )}
        <button
          type="button"
          className="link op-name"
          onClick={() => {
            dispatch({ kind: "select", name: entry.name });
            open({ kind: "operation", name: entry.name });
          }}
        >
          {entry.name}
        </button>
        <span className="badge kind">{entry.kind}</span>
        <span className="op-type">
          {entry.ty_name === "" ? "(no type)" : entry.ty_name}
        </span>
        <ReferenceChips entry={entry} entries={entries} index={index} />
        {problem !== null && (
          <span className="badge warn" title={problem}>
            invalid name
          </span>
        )}
        {findings !== undefined && judgment !== null && (
          <CheckStatus
            counts={countFindings(findings)}
            current={judgment.current}
          />
        )}
      </div>
      {warnings.map((warning) => (
        <p key={warning} className="op-warn" data-testid="op-warning">
          {warning}
        </p>
      ))}
      {findings !== undefined && judgment !== null && findings.length > 0 && (
        <FindingList
          findings={findings}
          names={judgment.names}
          current={judgment.current}
          note={false}
        />
      )}
    </li>
  );
}

export interface Row {
  index: number;
  entry: ProposalEntry;
  // The kept edges right after a revision, shown under it.
  group: { index: number; entry: ProposalEntry }[];
}

export function rowsOf(
  entries: readonly ProposalEntry[],
  kept: Record<string, KeptEdge>,
): Row[] {
  const rows: Row[] = [];
  let index = 0;
  while (index < entries.length) {
    const entry = entries[index] as ProposalEntry;
    const group: Row["group"] = [];
    let next = index + 1;
    while (entry.kind === "node") {
      const member = entries[next];
      if (
        member?.kind !== "edge" ||
        kept[member.name]?.revision !== entry.name
      ) {
        break;
      }
      group.push({ index: next, entry: member });
      next += 1;
    }
    rows.push({ index, entry, group });
    index = next;
  }
  return rows;
}

// Collapsed by default to one line; an edge with findings, warnings, or the
// selection shows anyway.
function KeptGroup({
  row,
  ...props
}: Omit<RowProps, "entry" | "index"> & { row: Row }) {
  const { state } = useDraft();
  const [expanded, setExpanded] = useState(false);
  const { entries } = state.proposal;
  const findings = row.group.flatMap(
    ({ entry }) => props.judgment?.findings(entry.name) ?? [],
  );
  const notable = row.group.filter(
    ({ entry, index }) =>
      entry.name === props.selected ||
      operationWarnings(entries, index, state.kept).length > 0 ||
      (props.judgment?.findings(entry.name)?.length ?? 0) > 0,
  );
  const shown = expanded ? row.group : notable;
  return (
    <li className="kept-group" data-testid="kept-group">
      <button
        type="button"
        className="link"
        aria-expanded={expanded}
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? "▾" : "▸"} {row.group.length} kept edge
        {row.group.length === 1 ? "" : "s"}
      </button>{" "}
      <span className="muted">of {row.entry.name}</span>
      {props.judgment !== null && findings.length > 0 && (
        <>
          {" "}
          <CheckStatus
            counts={countFindings(findings)}
            current={props.judgment.current}
          />
        </>
      )}
      {shown.length > 0 && (
        <ol className="operations nested">
          {shown.map(({ entry, index }) => (
            <OperationRow
              key={entry.name}
              entry={entry}
              index={index}
              {...props}
            />
          ))}
        </ol>
      )}
    </li>
  );
}

export function OperationRows({
  rows,
  ...props
}: Omit<RowProps, "entry" | "index"> & { rows: Row[] }) {
  return (
    <ol className="operations" data-testid="operations">
      {rows.map((row) => (
        <RowWithGroup key={row.entry.name} row={row} {...props} />
      ))}
    </ol>
  );
}

function RowWithGroup({
  row,
  ...props
}: Omit<RowProps, "entry" | "index"> & { row: Row }) {
  return (
    <>
      <OperationRow entry={row.entry} index={row.index} {...props} />
      {row.group.length > 0 && <KeptGroup row={row} {...props} />}
    </>
  );
}
