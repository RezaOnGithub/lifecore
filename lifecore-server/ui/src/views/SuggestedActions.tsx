import { useEffect } from "react";
import type {
  EntryChange,
  NodeRecord,
  Observation,
  Reference,
} from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { frozen, frozenReason } from "../draft/model.ts";
import { OperationLink } from "../links.tsx";
import { addBlocker, keyText } from "../model/findings.ts";
import { typeName } from "../model/labels.ts";
import { formatBody } from "../model/payload.ts";
import { counted } from "../model/words.ts";
import type { Ran } from "./analysisRuns.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";
import { EntryChip } from "./FindingRow.tsx";
import { useAnalysisRuns } from "./useAnalysisRuns.ts";

function ActionReference({
  reference,
  nodes,
}: {
  reference: Reference;
  nodes: Map<string, NodeRecord>;
}) {
  return "existing" in reference ? (
    <EntryChip id={reference.existing} nodes={nodes} />
  ) : (
    <span className="ref-chip">action {reference.added + 1}</span>
  );
}

// A suggestion's operations, read-only, and a way to start a draft from
// them. Added operations are ordinary ones: the proposal does not
// acknowledge the finding (PLAN §4.4). The draft applies to the current
// history, so only actions judged at the boundary of a whole-history run of
// this reload can be added; a second addition asks first.
export function SuggestedActions({
  observation,
  actions,
  ran,
}: {
  observation: Observation;
  actions: EntryChange[];
  ran: Ran;
}) {
  const { state, actions: draft } = useDraft();
  const { refresh } = useData();
  const store = useAnalysisRuns();
  const { nodes } = ran;
  const id = `${keyText(observation.key)}${JSON.stringify(observation.evaluated_view)}`;
  const recorded = store.added(id);
  const inDraft = new Set(state.proposal.entries.map((entry) => entry.name));
  const added = recorded?.filter((name) => inDraft.has(name)) ?? [];
  const gone = recorded !== null && added.length === 0;
  // Once none of them is in the draft, as after an apply or a clear, the
  // record goes, so a later operation of one of their names is not taken
  // for them.
  useEffect(() => {
    if (gone) {
      store.forgetAdded(id);
    }
  }, [store, id, gone]);
  if (actions.length === 0) {
    return <p className="muted">no suggested operations</p>;
  }
  const blocker = addBlocker(observation, ran.run.analysis, {
    whole: ran.params.as_of === undefined,
    current: ran.refresh === refresh,
  });
  const locked = frozen(state);
  const title =
    blocker ??
    frozenReason(state) ??
    "appends these operations to the draft, without acknowledging the finding";
  const add = () => {
    const names = draft.addSuggested(actions);
    if (names.length > 0) {
      store.setAdded(id, names);
    }
  };
  return (
    <div className="suggested">
      <ol className="suggested-actions" data-testid="suggested-actions">
        {actions.map((action, index) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: actions are positional
          <li key={index}>
            {action.kind === "node" ? (
              <>
                node{" "}
                <code title={formatBody(action.payload)}>
                  {typeName(action.ty_name)}
                </code>
                {action.predecessor !== undefined && (
                  <>
                    {" "}
                    {action.intent === "branch" ? "branches" : "revises"}{" "}
                    <ActionReference
                      reference={action.predecessor}
                      nodes={nodes}
                    />
                  </>
                )}
              </>
            ) : (
              <>
                edge <code>{typeName(action.ty_name)}</code>{" "}
                <ActionReference reference={action.from} nodes={nodes} /> →{" "}
                <ActionReference reference={action.to} nodes={nodes} />
              </>
            )}
          </li>
        ))}
      </ol>
      <div className="row" title={blocker ?? undefined}>
        {added.length === 0 ? (
          <button
            type="button"
            disabled={locked || blocker !== null}
            title={title}
            onClick={add}
          >
            Add to draft
          </button>
        ) : (
          <ConfirmButton
            confirm="Add them again?"
            disabled={locked || blocker !== null}
            title={title}
            onConfirm={add}
          >
            added ({counted(added.length, "operation")})
          </ConfirmButton>
        )}
        {added.length > 0 && (
          <span className="muted">
            {added.map((name) => (
              <span key={name}>
                <OperationLink name={name} />{" "}
              </span>
            ))}
          </span>
        )}
      </div>
    </div>
  );
}
