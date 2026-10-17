import type { ProposalReference } from "../contract.ts";
import { isUnset } from "../draft/model.ts";
import { Position } from "../links.tsx";

// `@position` for an existing entry, `+name` for one added by the draft.
export function ReferenceChip({ reference }: { reference: ProposalReference }) {
  return (
    <span className="ref-chip">
      {"added" in reference ? (
        <code>+{reference.added}</code>
      ) : isUnset(reference) ? (
        <span className="muted">not picked yet</span>
      ) : (
        <Position id={reference.existing} />
      )}
    </span>
  );
}
