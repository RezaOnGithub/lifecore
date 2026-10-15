import { useEffect, useState } from "react";
import type {
  Currentness,
  EntryListParams,
  NodeRecord,
  TypeResolution,
  TypeSummary,
} from "../contract.ts";
import { schemaOf, useAsOf, useCall, useTypes } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink } from "../links.tsx";
import { EntryTable } from "../views/EntryTable.tsx";

export function CurrentnessSelect({
  id,
  value,
  onChange,
  disabled = false,
}: {
  id?: string;
  value: Currentness;
  onChange: (value: Currentness) => void;
  disabled?: boolean;
}) {
  return (
    <select
      id={id}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value as Currentness)}
    >
      <option value="current">current</option>
      <option value="historical">historical</option>
      <option value="all">all</option>
    </select>
  );
}

export function ResolutionBadge({
  resolution,
}: {
  resolution: TypeResolution;
}) {
  switch (resolution.kind) {
    case "builtin":
    case "declared":
      return null;
    case "missing":
      return <span className="badge warn">missing</span>;
    case "ambiguous":
      return <span className="badge warn">ambiguous</span>;
    case "malformed":
      return (
        <span className="badge warn" title={resolution.message}>
          malformed
        </span>
      );
  }
}

function ResolutionNote({ summary }: { summary: TypeSummary | undefined }) {
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
  const rows =
    load.status === "ready"
      ? load.result.entries.filter(
          (entry): entry is NodeRecord => entry.kind === "node",
        )
      : [];
  return (
    <section data-testid="table">
      <h2>{ty_name === "" ? <em>(unnamed)</em> : ty_name}</h2>
      <ResolutionNote summary={types?.find((type) => type.name === ty_name)} />
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
        {load.status === "ready" && (
          <span className="muted">
            {rows.length} row{rows.length === 1 ? "" : "s"}
          </span>
        )}
      </div>
      {load.status === "loading" && <p className="muted">loading…</p>}
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {load.status === "ready" && (
        <EntryTable
          rows={rows}
          schema={schemaOf(types, ty_name)}
          ty_name={ty_name}
        />
      )}
    </section>
  );
}
