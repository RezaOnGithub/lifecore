import { useEffect, useState } from "react";
import type {
  Currentness,
  EntryListParams,
  NodeRecord,
  TypeSummary,
} from "../contract.ts";
import { latest, schemaOf, useAsOf, useCall, useTypes } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, useOpen } from "../links.tsx";
import { typeName } from "../model/labels.ts";
import { EntryTable } from "../views/EntryTable.tsx";
import { CurrentnessSelect, ResolutionBadge } from "../views/widgets.tsx";

// Nothing until the type list has loaded: an unknown name is only known to
// be unknown once the list is there.
function ResolutionNote({
  types,
  ty_name,
}: {
  types: TypeSummary[] | null;
  ty_name: string;
}) {
  if (types === null) {
    return null;
  }
  const summary = types.find((type) => type.name === ty_name);
  if (summary === undefined) {
    return <p className="muted">This type name is unknown to the view.</p>;
  }
  const { resolution } = summary;
  switch (resolution.kind) {
    case "builtin":
      return (
        <p className="muted">The built-in metatype; rows are declarations.</p>
      );
    case "declared":
      return (
        <p className="muted">
          Declared by <IdLink id={resolution.declaration} />.
        </p>
      );
    case "missing":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} /> No current declaration;
          nodes use the name anyway.
        </p>
      );
    case "ambiguous":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} />{" "}
          {resolution.declarations.length} current declarations:{" "}
          {resolution.declarations.map((id, index) => (
            <span key={id.position}>
              {index > 0 && ", "}
              <IdLink id={id} />
            </span>
          ))}
        </p>
      );
    case "malformed":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} /> Declaration{" "}
          <IdLink id={resolution.declaration} /> does not parse:{" "}
          {resolution.message}
        </p>
      );
  }
}

export function TableScreen({ ty_name }: { ty_name: string }) {
  const asOf = useAsOf();
  const types = useTypes();
  const { actions } = useDraft();
  const open = useOpen();
  const [currentness, setCurrentness] = useState<Currentness>("current");
  const [text, setText] = useState("");
  const [applied, setApplied] = useState("");
  // The filter goes to the backend; a short pause keeps typing from sending a
  // request per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => setApplied(text.trim()), 300);
    return () => clearTimeout(timer);
  }, [text]);
  const params: EntryListParams = {
    ...asOf,
    kind: "node",
    ty_name,
    currentness,
  };
  if (applied !== "") {
    params.text = applied;
  }
  const load = useCall("entries.list", params);
  // A reload keeps the last rows, and the sort, on screen until the new ones
  // arrive.
  const result = latest(load);
  const rows =
    result === null
      ? []
      : result.entries.filter(
          (entry): entry is NodeRecord => entry.kind === "node",
        );
  return (
    <section data-testid="table">
      <h2>
        {typeName(ty_name)}{" "}
        <button
          type="button"
          onClick={() =>
            open({ kind: "operation", name: actions.addNode(ty_name) })
          }
        >
          New node of this type
        </button>
      </h2>
      <ResolutionNote types={types} ty_name={ty_name} />
      <div className="row">
        <label htmlFor="table-currentness">show</label>
        <CurrentnessSelect
          id="table-currentness"
          value={currentness}
          onChange={setCurrentness}
        />
        <label>
          containing{" "}
          <input
            type="search"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="any text"
          />
        </label>
        {result !== null && (
          <>
            <span className="muted">
              {rows.length} row{rows.length === 1 ? "" : "s"}
            </span>
            <span className="muted">view [0, {result.view.end_exclusive})</span>
          </>
        )}
        {load.status === "loading" && (
          <span className="muted">
            {result === null ? "loading…" : "reloading…"}
          </span>
        )}
      </div>
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {result !== null && (
        <EntryTable
          rows={rows}
          schema={schemaOf(types, ty_name)}
          ty_name={ty_name}
        />
      )}
    </section>
  );
}
