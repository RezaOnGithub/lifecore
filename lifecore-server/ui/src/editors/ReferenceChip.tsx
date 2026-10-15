import type { ProposalReference } from "../contract.ts";
import { Position } from "../links.tsx";

// `@position` for an existing entry, `+name` for one added by the draft.
export function ReferenceChip({ reference }: { reference: ProposalReference }) {
  return (
    <span className="ref-chip">
      {"existing" in reference ? (
        <Position id={reference.existing} />
      ) : (
        <code>+{reference.added}</code>
      )}
    </span>
  );
}
