import { useId, useState } from "react";
import type { ProposalReference } from "../contract.ts";
import { schemaOf } from "../data.tsx";
import {
  addedNodeType,
  type EdgeOperation,
  earlierNodes,
  edgeTypeSuggestions,
  isUnpicked,
  type NodeOperation,
  sameReference,
} from "./helpers.ts";
import type { OperationEditorProps, PickRole } from "./OperationEditor.tsx";
import { ReferenceChip } from "./ReferenceChip.tsx";

const EXISTING = "existing";
const UNKNOWN = "unknown";
const ADDED = "added:";

// One endpoint: an earlier node operation by name, or an existing entry
// picked from the inspector. Choosing "existing…" over an `added` reference
// keeps that reference until an entry is picked: there is no blank
// identity to put in its place. An existing entry is only picked, never
// typed: its identity includes the database origin, which no one types.
function EndpointPicker({
  end,
  reference,
  earlier,
  onChange,
  onPick,
}: {
  end: Extract<PickRole, "from" | "to">;
  reference: ProposalReference;
  earlier: NodeOperation[];
  onChange: (reference: ProposalReference) => void;
  onPick: () => void;
}) {
  const id = useId();
  const [pendingExisting, setPendingExisting] = useState(false);
  const added = "added" in reference ? reference.added : null;
  const known =
    added !== null && earlier.some((operation) => operation.name === added);
  const existingMode = added === null || pendingExisting;
  const selected = existingMode
    ? EXISTING
    : known
      ? `${ADDED}${added}`
      : UNKNOWN;
  return (
    <div className="row">
      <label htmlFor={`${id}-${end}`}>{end}</label>
      <select
        id={`${id}-${end}`}
        value={selected}
        onChange={(event) => {
          const choice = event.target.value;
          if (choice === EXISTING) {
            setPendingExisting(true);
          } else if (choice.startsWith(ADDED)) {
            setPendingExisting(false);
            onChange({ added: choice.slice(ADDED.length) });
          }
        }}
      >
        <option value={EXISTING}>existing…</option>
        {earlier.map((operation) => (
          <option key={operation.name} value={`${ADDED}${operation.name}`}>
            {operation.name}
          </option>
        ))}
        {added !== null && !known && (
          <option value={UNKNOWN} disabled>
            +{added} (not an earlier node operation)
          </option>
        )}
      </select>
      {existingMode && (
        <>
          {"existing" in reference ? (
            <ReferenceChip reference={reference} />
          ) : (
            <span className="muted">
              still +{added} until an entry is picked
            </span>
          )}
          <button type="button" onClick={onPick}>
            pick {end}
          </button>
        </>
      )}
      {added !== null && !known && (
        <span className="field-error">
          {end} +{added} is not an earlier node operation
        </span>
      )}
    </div>
  );
}

export function EdgeEditor({
  entry,
  operations,
  index,
  types,
  onChange,
  onPick,
}: Omit<OperationEditorProps, "entry"> & { entry: EdgeOperation }) {
  const id = useId();
  const earlier = earlierNodes(operations, index);
  // Only an `added` end's type is known here: an existing entry's type
  // would take a read, so it counts as unknown.
  const fromType = addedNodeType(entry.from, earlier);
  const toType = addedNodeType(entry.to, earlier);
  const suggestions = edgeTypeSuggestions(
    fromType === null ? null : schemaOf(types, fromType),
    toType === null ? null : schemaOf(types, toType),
    types,
  );
  const loop = !isUnpicked(entry.from) && sameReference(entry.from, entry.to);
  return (
    <>
      <div className="row">
        <label htmlFor={`${id}-type`}>edge type</label>
        <input
          id={`${id}-type`}
          type="text"
          size={24}
          list={`${id}-labels`}
          value={entry.ty_name}
          onChange={(event) =>
            onChange({ ...entry, ty_name: event.target.value })
          }
        />
        <datalist id={`${id}-labels`}>
          {suggestions.labels.map((label) => (
            <option key={label} value={label} />
          ))}
        </datalist>
        {suggestions.note !== null && (
          <span className="edit-hint">{suggestions.note}</span>
        )}
      </div>
      <EndpointPicker
        end="from"
        reference={entry.from}
        earlier={earlier}
        onChange={(from) => onChange({ ...entry, from })}
        onPick={() => onPick("from")}
      />
      <EndpointPicker
        end="to"
        reference={entry.to}
        earlier={earlier}
        onChange={(to) => onChange({ ...entry, to })}
        onPick={() => onPick("to")}
      />
      {loop && (
        <p className="edit-hint">from and to are the same node: a self-loop</p>
      )}
    </>
  );
}
