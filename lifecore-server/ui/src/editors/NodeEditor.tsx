import { useId, useState } from "react";
import type { RevisionIntent } from "../contract.ts";
import { schemaOf } from "../data.tsx";
import { typeName } from "../model/labels.ts";
import { ResolutionBadge } from "../views/widgets.tsx";
import { earlierNodes, type NodeOperation, typeNames } from "./helpers.ts";
import type { OperationEditorProps } from "./OperationEditor.tsx";
import { PayloadEditor } from "./PayloadEditor.tsx";
import { ReferenceChip } from "./ReferenceChip.tsx";
import { TypeDeclarationEditor } from "./TypeDeclarationEditor.tsx";

// The select's value for "other…": no type can be named this.
const OTHER = "\u0000other";

// A root again: no predecessor, and so no intent (FILES §2.4).
function asRoot(entry: NodeOperation): NodeOperation {
  return {
    name: entry.name,
    kind: "node",
    ty_name: entry.ty_name,
    payload: entry.payload,
  };
}

export function NodeEditor({
  entry,
  operations,
  index,
  types,
  onChange,
  onPick,
}: Omit<OperationEditorProps, "entry"> & { entry: NodeOperation }) {
  const id = useId();
  // "other…" stays chosen while the typed name happens to match a listed
  // type, so the text input does not vanish mid-edit.
  const [forcedOther, setForcedOther] = useState(false);
  const names = typeNames(types);
  const other = forcedOther || !names.includes(entry.ty_name);
  const summary = types?.find((type) => type.name === entry.ty_name);
  const schema = schemaOf(types, entry.ty_name);
  const setIntent = (intent: RevisionIntent) => onChange({ ...entry, intent });
  const { predecessor, intent } = entry;
  const earlier = earlierNodes(operations, index);
  const predecessorProblem =
    predecessor !== undefined &&
    "added" in predecessor &&
    !earlier.some((operation) => operation.name === predecessor.added)
      ? `+${predecessor.added} is not an earlier node operation`
      : null;
  return (
    <>
      <div className="row">
        <label htmlFor={`${id}-type`}>type</label>
        <select
          id={`${id}-type`}
          value={other ? OTHER : entry.ty_name}
          onChange={(event) => {
            if (event.target.value === OTHER) {
              setForcedOther(true);
            } else {
              setForcedOther(false);
              onChange({ ...entry, ty_name: event.target.value });
            }
          }}
        >
          {names.map((name) => (
            <option key={name} value={name}>
              {typeName(name)}
            </option>
          ))}
          <option value={OTHER}>other…</option>
        </select>
        {other && (
          <>
            <label htmlFor={`${id}-type-name`}>type name</label>
            <input
              id={`${id}-type-name`}
              type="text"
              size={20}
              value={entry.ty_name}
              onChange={(event) => {
                setForcedOther(true);
                onChange({ ...entry, ty_name: event.target.value });
              }}
            />
          </>
        )}
        {summary === undefined ? (
          types !== null &&
          entry.ty_name !== "Type" && (
            <span className="badge warn">undeclared</span>
          )
        ) : (
          <ResolutionBadge resolution={summary.resolution} />
        )}
      </div>
      <div className="row">
        <span>predecessor</span>
        {predecessor ? (
          <ReferenceChip reference={predecessor} />
        ) : (
          <span className="muted">none (root)</span>
        )}
        <button type="button" onClick={() => onPick("predecessor")}>
          pick predecessor
        </button>
        <button
          type="button"
          disabled={predecessor === undefined}
          onClick={() => onChange(asRoot(entry))}
        >
          clear
        </button>
        {predecessorProblem !== null && (
          <span className="field-error">{predecessorProblem}</span>
        )}
        {predecessor === undefined && intent !== undefined && (
          <>
            <span className="field-error">
              a root has no intent, but this one says {intent}
            </span>
            <button type="button" onClick={() => onChange(asRoot(entry))}>
              remove intent
            </button>
          </>
        )}
        {predecessor && (
          <span className="intent">
            <label>
              <input
                type="radio"
                name={`${id}-intent`}
                value="revise"
                checked={intent === "revise"}
                onChange={() => setIntent("revise")}
              />{" "}
              revise
            </label>
            <label>
              <input
                type="radio"
                name={`${id}-intent`}
                value="branch"
                checked={intent === "branch"}
                onChange={() => setIntent("branch")}
              />{" "}
              branch
            </label>
            {intent === undefined && (
              <span className="field-error">intent required</span>
            )}
          </span>
        )}
      </div>
      {entry.ty_name === "Type" ? (
        <TypeDeclarationEditor
          payload={entry.payload}
          onChange={(payload) => onChange({ ...entry, payload })}
        />
      ) : (
        <PayloadEditor
          payload={entry.payload}
          schema={schema}
          onChange={(payload) => onChange({ ...entry, payload })}
        />
      )}
    </>
  );
}
