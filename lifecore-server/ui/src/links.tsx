import { createContext, useContext } from "react";
import type { EntryId, NodeEntry } from "./contract.ts";
import { labelField, rowLabel } from "./model/labels.ts";
import type { Screen } from "./navigation.ts";

export type Open = (screen: Screen) => void;

export const NavigationContext = createContext<Open>(() => {
  throw new Error("NavigationContext has no provider");
});

export function useOpen(): Open {
  return useContext(NavigationContext);
}

export function Position({ id }: { id: EntryId }) {
  return <code className="pos">@{id.position}</code>;
}

// A node's label linking to its inspector. `@position` follows it unless the
// fallback label already carries the position.
export function NodeLink({
  node,
  position = true,
}: {
  node: NodeEntry;
  position?: boolean;
}) {
  const open = useOpen();
  return (
    <>
      <button
        type="button"
        className="link"
        onClick={() => open({ kind: "inspector", id: node.id })}
      >
        {rowLabel(node)}
      </button>
      {position && labelField(node) !== null && (
        <>
          {" "}
          <Position id={node.id} />
        </>
      )}
    </>
  );
}

// A bare identity linking to its inspector, for edges and declarations.
export function IdLink({ id }: { id: EntryId }) {
  const open = useOpen();
  return (
    <button
      type="button"
      className="link"
      onClick={() => open({ kind: "inspector", id })}
    >
      <Position id={id} />
    </button>
  );
}

export function TypeLink({ name }: { name: string }) {
  const open = useOpen();
  return (
    <button
      type="button"
      className="link"
      onClick={() => open({ kind: "table", ty_name: name })}
    >
      {name}
    </button>
  );
}

// A superseded node has a successor in the view; a historical edge has a
// superseded endpoint.
export function CurrentBadge({
  current,
  kind,
}: {
  current: boolean;
  kind: "node" | "edge";
}) {
  const word = current
    ? "current"
    : kind === "node"
      ? "superseded"
      : "historical";
  return (
    <span className={`badge ${current ? "current" : "historical"}`}>
      {word}
    </span>
  );
}
