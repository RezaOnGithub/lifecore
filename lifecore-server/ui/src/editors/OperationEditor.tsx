import { useId, useState } from "react";
import type { ProposalEntry, TypeSummary } from "../contract.ts";
import { EdgeEditor } from "./EdgeEditor.tsx";
import { liveDraft, operationNameError, type TextDraft } from "./helpers.ts";
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
  // Names identify operations, so only a valid, unique name is committed;
  // other text stays here with its error until focus leaves, which puts
  // the committed name back.
  const [draft, setDraft] = useState<TextDraft | null>(null);
  const [note, setNote] = useState<TextDraft | null>(null);
  const live = liveDraft(draft, entry.name);
  const text = live?.text ?? entry.name;
  const problem = operationNameError(text, operations, index);
  const shownNote = liveDraft(note, entry.name)?.text ?? null;
  return (
    <div className="editor" data-testid="operation-editor">
      <div className="row">
        <label htmlFor={`${id}-name`}>name</label>
        <input
          id={`${id}-name`}
          type="text"
          size={24}
          value={text}
          aria-invalid={problem !== null}
          onChange={(event) => {
            const next = event.target.value;
            setNote(null);
            if (next === entry.name) {
              setDraft(null);
            } else if (operationNameError(next, operations, index) === null) {
              setDraft(null);
              onChange({ ...entry, name: next });
            } else {
              setDraft({ text: next, base: entry.name });
            }
          }}
          onBlur={() => {
            if (live !== null) {
              setDraft(null);
              setNote({ text: `reverted to ${entry.name}`, base: entry.name });
            }
          }}
        />
        {problem !== null && <span className="field-error">{problem}</span>}
        {shownNote !== null && <span className="edit-note">{shownNote}</span>}
      </div>
      {entry.kind === "node" ? (
        <NodeEditor {...props} entry={entry} />
      ) : (
        <EdgeEditor {...props} entry={entry} />
      )}
    </div>
  );
}
