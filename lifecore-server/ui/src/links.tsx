import { createContext, useContext } from "react";
import type { EntryId, NodeEntry } from "./contract.ts";
import { useEntryNames } from "./data.tsx";
import { LABEL_LIMIT, labelField, rowLabel, typeName } from "./model/labels.ts";
import { truncate } from "./model/payload.ts";
import type { ExploreForm } from "./model/query.ts";
import type { Screen } from "./navigation.ts";

// Opens a screen as a new Back step. `leaving` replaces the screen being
// left first, as when a run leaves the previous query as it was run.
export type Open = (screen: Screen, options?: { leaving?: Screen }) => void;

export const NavigationContext = createContext<Open>(() => {
  throw new Error("NavigationContext has no provider");
});

export function useOpen(): Open {
  return useContext(NavigationContext);
}

// Swaps the current screen without a Back step, as when the operation under
// the editor is renamed.
export const ReplaceContext = createContext<(screen: Screen) => void>(() => {
  throw new Error("ReplaceContext has no provider");
});

export function useReplace(): (screen: Screen) => void {
  return useContext(ReplaceContext);
}

// Changes what the step a screen is shown for remembers: its view state.
// Bound to that step, so it does nothing once the step is left; a screen
// shown while another is current, like the hidden Explore, changes nothing.
export type Edit = (edit: (screen: Screen) => Screen) => void;

export const EditContext = createContext<Edit>(() => {});

export function useEdit(): Edit {
  return useContext(EditContext);
}

// Shows a form's query in Explore as a new Back step, with the display
// choices of the last Explore step; on Explore itself, a query already on
// show is run again instead.
export const ExploreContext = createContext<(form: ExploreForm) => void>(() => {
  throw new Error("ExploreContext has no provider");
});

export function useExplore(): (form: ExploreForm) => void {
  return useContext(ExploreContext);
}

// `@position`, then the names kept for the entry in this browser, if any;
// none is shown for an entry past the as-of boundary.
export function Position({ id }: { id: EntryId }) {
  const names = useEntryNames(id);
  return (
    <>
      <code className="pos">@{id.position}</code>
      {names.length > 0 && (
        <>
          {" "}
          <span className="alias" title="names kept in this browser">
            {names.join(", ")}
          </span>
        </>
      )}
    </>
  );
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
  const label = rowLabel(node);
  return (
    <>
      <button
        type="button"
        className="link"
        title={label.length > LABEL_LIMIT ? label : undefined}
        onClick={() => open({ kind: "inspector", id: node.id })}
      >
        {truncate(label, LABEL_LIMIT)}
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

// A draft operation linking to its editor, read "+name" as in the reference
// chips.
export function OperationLink({ name }: { name: string }) {
  const open = useOpen();
  return (
    <button
      type="button"
      className="link"
      onClick={() => open({ kind: "operation", name })}
    >
      +{name}
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
      {typeName(name)}
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
