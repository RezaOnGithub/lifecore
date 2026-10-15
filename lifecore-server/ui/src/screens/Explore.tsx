import { type FormEvent, useState } from "react";
import type {
  Direction,
  EntryId,
  NodeRecord,
  Provenance,
  TypeSummary,
  ViewQuery,
  ViewQueryParams,
} from "../contract.ts";
import { schemaOf, useAsOf, useCall, useTypes } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, TypeLink } from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey } from "../model/labels.ts";
import {
  defaultExpansion,
  type ExpansionForm,
  type ExploreForm,
  removeSeed,
  toParams,
} from "../model/query.ts";
import { EntryTable, type ExtraColumn } from "../views/EntryTable.tsx";
import { ThreadView } from "../views/Thread.tsx";
import { CurrentnessSelect } from "./Table.tsx";

// Edge type names the declarations permit, as suggestions; any name may be
// typed, since edges of undeclared types can exist.
function edgeTypeNames(types: TypeSummary[] | null): string[] {
  const names = new Set<string>();
  for (const type of types ?? []) {
    if (type.resolution.kind === "declared") {
      for (const name of type.resolution.schema.outgoing) {
        names.add(name);
      }
      for (const name of type.resolution.schema.incoming) {
        names.add(name);
      }
    }
  }
  return [...names].sort(compareStrings);
}

function DirectionSelect({
  id,
  value,
  onChange,
}: {
  id: string;
  value: Direction;
  onChange: (value: Direction) => void;
}) {
  return (
    <select
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value as Direction)}
    >
      <option value="both">both</option>
      <option value="outgoing">outgoing</option>
      <option value="incoming">incoming</option>
    </select>
  );
}

function ExpansionRow({
  step,
  index,
  removable,
  onChange,
  onRemove,
}: {
  step: ExpansionForm;
  index: number;
  removable: boolean;
  onChange: (step: ExpansionForm) => void;
  onRemove: () => void;
}) {
  return (
    <div className="row">
      <span className="muted">step {index + 1}</span>
      <label>
        hops{" "}
        <input
          type="number"
          min={1}
          max={3}
          value={step.hops}
          onChange={(event) => {
            const hops = Number(event.target.value);
            if (Number.isInteger(hops) && hops >= 1 && hops <= 3) {
              onChange({ ...step, hops });
            }
          }}
        />
      </label>
      <label htmlFor={`step-${index}-direction`}>direction</label>
      <DirectionSelect
        id={`step-${index}-direction`}
        value={step.direction}
        onChange={(direction) => onChange({ ...step, direction })}
      />
      <label>
        edge types{" "}
        <input
          list="edge-types"
          value={step.ty_names}
          placeholder="any"
          onChange={(event) =>
            onChange({ ...step, ty_names: event.target.value })
          }
        />
      </label>
      <label htmlFor={`step-${index}-currentness`}>edges</label>
      <CurrentnessSelect
        id={`step-${index}-currentness`}
        value={step.currentness}
        onChange={(currentness) => onChange({ ...step, currentness })}
      />
      <button type="button" disabled={!removable} onClick={onRemove}>
        remove
      </button>
    </div>
  );
}

type QueriedRow = NodeRecord & { provenance: Provenance };

function Via({
  provenance,
  result,
}: {
  provenance: Provenance;
  result: ViewQuery;
}) {
  if (provenance.kind === "seed") {
    return <span>seed</span>;
  }
  const edge = result.edges.find(
    (candidate) => entryKey(candidate.id) === entryKey(provenance.via),
  );
  const from = result.nodes.find(
    (candidate) => entryKey(candidate.node.id) === entryKey(provenance.from),
  );
  return (
    <span>
      hop {provenance.hop} ←{" "}
      {edge ? (
        edge.ty_name
      ) : (
        <>
          edge <IdLink id={provenance.via} />
        </>
      )}{" "}
      from{" "}
      {from ? <NodeLink node={from.node} /> : <IdLink id={provenance.from} />}
    </span>
  );
}

function GroupedTables({ result }: { result: ViewQuery }) {
  const types = useTypes();
  const groups = new Map<string, QueriedRow[]>();
  for (const { node, provenance } of result.nodes) {
    const rows = groups.get(node.ty_name) ?? [];
    rows.push({ ...node, provenance });
    groups.set(node.ty_name, rows);
  }
  const incident = new Map<string, number>();
  for (const edge of result.edges) {
    for (const end of [edge.from, edge.to]) {
      const key = entryKey(end);
      incident.set(key, (incident.get(key) ?? 0) + 1);
    }
  }
  const extra: ExtraColumn<QueriedRow>[] = [
    {
      key: "via",
      header: "via",
      cell: (row) => <Via provenance={row.provenance} result={result} />,
      sortValue: (row) => ({
        kind: "integer",
        value: String(row.provenance.kind === "seed" ? 0 : row.provenance.hop),
      }),
    },
    {
      key: "edges",
      header: "edges",
      cell: (row) => incident.get(entryKey(row.id)) ?? 0,
      sortValue: (row) => ({
        kind: "integer",
        value: String(incident.get(entryKey(row.id)) ?? 0),
      }),
    },
  ];
  return (
    <>
      {[...groups.keys()].sort(compareStrings).map((ty_name) => {
        const rows = groups.get(ty_name) ?? [];
        return (
          <section key={ty_name}>
            <h3>
              <TypeLink name={ty_name} />{" "}
              <span className="muted">
                {rows.length} node{rows.length === 1 ? "" : "s"}
              </span>
            </h3>
            <EntryTable
              rows={rows}
              schema={schemaOf(types, ty_name)}
              ty_name={ty_name}
              extra={extra}
            />
          </section>
        );
      })}
    </>
  );
}

export function ExploreScreen({
  form,
  onChange,
}: {
  form: ExploreForm;
  onChange: (form: ExploreForm) => void;
}) {
  const asOf = useAsOf();
  const types = useTypes();
  const [submitted, setSubmitted] = useState<ViewQueryParams | null>(null);
  const [runs, setRuns] = useState(0);
  const [display, setDisplay] = useState<"table" | "thread">("table");
  const load = useCall(
    "views.query",
    submitted === null ? null : { ...asOf, ...submitted },
    runs,
  );
  const query = toParams(form);
  const run = (event: FormEvent) => {
    event.preventDefault();
    if (query !== null) {
      setSubmitted(query);
      setRuns((count) => count + 1);
    }
  };
  const setStep = (index: number, step: ExpansionForm) =>
    onChange({
      ...form,
      expand: form.expand.map((existing, i) => (i === index ? step : existing)),
    });
  return (
    <section>
      <h2>Explore</h2>
      <form className="explore" onSubmit={run}>
        <fieldset>
          <legend>Seeds</legend>
          <ul className="chips" data-testid="seeds">
            {form.ids.map((id: EntryId) => (
              <li key={entryKey(id)}>
                <IdLink id={id} />{" "}
                <button
                  type="button"
                  className="link"
                  aria-label={`remove @${id.position}`}
                  onClick={() => onChange(removeSeed(form, id))}
                >
                  ×
                </button>
              </li>
            ))}
            {form.ids.length === 0 && (
              <li className="muted">
                no entry seeds; the inspector's "Explore from here" adds one
              </li>
            )}
          </ul>
          <div className="row">
            <label>
              <input
                type="checkbox"
                checked={form.nodes.enabled}
                onChange={(event) =>
                  onChange({
                    ...form,
                    nodes: { ...form.nodes, enabled: event.target.checked },
                  })
                }
              />{" "}
              nodes of type
            </label>
            <select
              value={form.nodes.ty_name}
              disabled={!form.nodes.enabled}
              onChange={(event) =>
                onChange({
                  ...form,
                  nodes: { ...form.nodes, ty_name: event.target.value },
                })
              }
            >
              <option value="">any type</option>
              {(types ?? []).map((type) => (
                <option key={type.name} value={type.name}>
                  {type.name === "" ? "(unnamed)" : type.name}
                </option>
              ))}
            </select>
            <label>
              containing{" "}
              <input
                type="search"
                value={form.nodes.text}
                disabled={!form.nodes.enabled}
                placeholder="any text"
                onChange={(event) =>
                  onChange({
                    ...form,
                    nodes: { ...form.nodes, text: event.target.value },
                  })
                }
              />
            </label>
            <CurrentnessSelect
              value={form.nodes.currentness}
              disabled={!form.nodes.enabled}
              onChange={(currentness) =>
                onChange({ ...form, nodes: { ...form.nodes, currentness } })
              }
            />
          </div>
        </fieldset>
        <fieldset>
          <legend>Expand</legend>
          {form.expand.map((step, index) => (
            <ExpansionRow
              // Steps are positional; reordering them is not offered.
              // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
              key={index}
              step={step}
              index={index}
              removable={form.expand.length > 1}
              onChange={(changed) => setStep(index, changed)}
              onRemove={() =>
                onChange({
                  ...form,
                  expand: form.expand.filter((_, i) => i !== index),
                })
              }
            />
          ))}
          <button
            type="button"
            disabled={form.expand.length >= 3}
            onClick={() =>
              onChange({
                ...form,
                expand: [...form.expand, defaultExpansion()],
              })
            }
          >
            + step
          </button>
        </fieldset>
        <fieldset>
          <legend>Edges among the results</legend>
          <div className="row">
            <label>
              edge types{" "}
              <input
                list="edge-types"
                value={form.edges.ty_names}
                placeholder="any"
                onChange={(event) =>
                  onChange({
                    ...form,
                    edges: { ...form.edges, ty_names: event.target.value },
                  })
                }
              />
            </label>
            <CurrentnessSelect
              value={form.edges.currentness}
              onChange={(currentness) =>
                onChange({ ...form, edges: { ...form.edges, currentness } })
              }
            />
          </div>
        </fieldset>
        <datalist id="edge-types">
          {edgeTypeNames(types).map((name) => (
            <option key={name} value={name} />
          ))}
        </datalist>
        <div className="row">
          <button type="submit" disabled={query === null}>
            Run
          </button>
          {query === null && <span className="muted">add a seed to run</span>}
        </div>
      </form>
      {load.status === "loading" && <p className="muted">running…</p>}
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {load.status === "ready" && (
        <section data-testid="results">
          <div className="row">
            <strong>
              {load.result.nodes.length} node
              {load.result.nodes.length === 1 ? "" : "s"},{" "}
              {load.result.edges.length} edge
              {load.result.edges.length === 1 ? "" : "s"}
            </strong>
            <span className="toggle">
              <button
                type="button"
                className={display === "table" ? "on" : undefined}
                onClick={() => setDisplay("table")}
              >
                Table
              </button>
              <button
                type="button"
                className={display === "thread" ? "on" : undefined}
                onClick={() => setDisplay("thread")}
              >
                Thread
              </button>
            </span>
            <span className="muted">
              view [0, {load.result.view.end_exclusive})
            </span>
          </div>
          {display === "table" ? (
            <GroupedTables result={load.result} />
          ) : (
            <ThreadView
              nodes={load.result.nodes.map((queried) => queried.node)}
              edges={load.result.edges}
            />
          )}
        </section>
      )}
    </section>
  );
}
