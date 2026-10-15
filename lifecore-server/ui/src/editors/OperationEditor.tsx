import { useId } from "react";
import type { ProposalEntry, TypeSummary } from "../contract.ts";
import { EdgeEditor } from "./EdgeEditor.tsx";
import { operationNameError } from "./helpers.ts";
import { NodeEditor } from "./NodeEditor.tsx";

// The role an existing entry is picked for from the Inspector.
export type PickRole = "predecessor" | "from" | "to";

// A controlled editor for one draft operation. The tray owns the draft and
// replaces the operation on every change; the editor never reads the draft
// or the network itself. `operations` is the whole draft in order and `index`
// the position of `entry` in it, so `added` references can be offered only to
// earlier node operations.
export interface OperationEditorProps {
  entry: ProposalEntry;
  operations: ProposalEntry[];
  index: number;
  types: TypeSummary[] | null;
  onChange: (entry: ProposalEntry) => void;
  onPick: (role: PickRole) => void;
}

export function OperationEditor(props: OperationEditorProps) {
  const { entry, operations, index, onChange } = props;
  const id = useId();
  // A bad name is still committed, so the tray shows what the user typed.
  const nameProblem = operationNameError(entry.name, operations, index);
  return (
    <div className="editor" data-testid="operation-editor">
      <div className="row">
        <label htmlFor={`${id}-name`}>name</label>
        <input
          id={`${id}-name`}
          type="text"
          size={24}
          value={entry.name}
          aria-invalid={nameProblem !== null}
          onChange={(event) => onChange({ ...entry, name: event.target.value })}
        />
        {nameProblem !== null && (
          <span className="field-error">{nameProblem}</span>
        )}
        <span className="badge">{entry.kind}</span>
        <span className="muted">
          operation {index + 1} of {operations.length}
        </span>
      </div>
      {entry.kind === "node" ? (
        <NodeEditor {...props} entry={entry} />
      ) : (
        <EdgeEditor {...props} entry={entry} />
      )}
    </div>
  );
}
