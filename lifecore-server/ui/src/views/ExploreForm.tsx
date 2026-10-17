import type { Direction, EntryId, TypeSummary } from "../contract.ts";
import { useTypes } from "../data.tsx";
import { Position, useOpen } from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  defaultExpansion,
  type ExpansionForm,
  type ExploreForm,
  MAX_HOPS,
  MAX_STEPS,
  removeSeed,
} from "../model/query.ts";
import { NEIGHBOUR_LIMIT } from "./RelationColumns.tsx";
import { CurrentnessSelect } from "./widgets.tsx";

interface FormProps {
  form: ExploreForm;
  onChange: (form: ExploreForm) => void;
}

// The type select's value for any type; a type name cannot clash with it,
// the empty one included, because the select lists names only as
// `type:<name>`.
const ANY_TYPE = "any";

// A seed chip: the node's label and position, linking to its inspector.
function SeedChip({ id, label }: { id: EntryId; label: string | undefined }) {
  const open = useOpen();
  return (
    <button
      type="button"
      className="link"
      title={label === undefined ? undefined : `${label} @${id.position}`}
      onClick={() => open({ kind: "inspector", id })}
    >
      {label === undefined ? (
        <Position id={id} />
      ) : (
        <>
          {truncate(label, NEIGHBOUR_LIMIT)} <Position id={id} />
        </>
      )}
    </button>
  );
}

export function SeedsFieldset({
  form,
  onChange,
  label,
}: FormProps & { label: (id: EntryId) => string | undefined }) {
  const types = useTypes();
  return (
    <fieldset>
      <legend>Seeds</legend>
      <ul className="chips" data-testid="seeds">
        {form.ids.map((id: EntryId) => (
          <li key={entryKey(id)}>
            <SeedChip id={id} label={label(id)} />{" "}
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
          aria-label="seed node type"
          value={
            form.nodes.ty_name === null
              ? ANY_TYPE
              : `type:${form.nodes.ty_name}`
          }
          disabled={!form.nodes.enabled}
          onChange={(event) => {
            const { value } = event.target;
            onChange({
              ...form,
              nodes: {
                ...form.nodes,
                ty_name: value.startsWith("type:")
                  ? value.slice("type:".length)
                  : null,
              },
            });
          }}
        >
          <option value={ANY_TYPE}>any type</option>
          {(types ?? []).map((type) => (
            <option key={type.name} value={`type:${type.name}`}>
              {typeName(type.name)}
            </option>
          ))}
          {/* A saved view may name a type this view does not list, or the
              list may still be loading; the select still shows the form's
              choice. */}
          {form.nodes.ty_name !== null &&
            !types?.some((type) => type.name === form.nodes.ty_name) && (
              <option value={`type:${form.nodes.ty_name}`}>
                {typeName(form.nodes.ty_name)}
                {types !== null && " (not in this view)"}
              </option>
            )}
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
        <label htmlFor="seed-currentness">seed nodes</label>
        <CurrentnessSelect
          id="seed-currentness"
          value={form.nodes.currentness}
          disabled={!form.nodes.enabled}
          onChange={(currentness) =>
            onChange({ ...form, nodes: { ...form.nodes, currentness } })
          }
        />
      </div>
    </fieldset>
  );
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

const HOPS = Array.from({ length: MAX_HOPS }, (_, index) => index + 1);

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
      <label htmlFor={`step-${index}-hops`}>hops</label>
      <select
        id={`step-${index}-hops`}
        value={step.hops}
        onChange={(event) =>
          onChange({ ...step, hops: Number(event.target.value) })
        }
      >
        {HOPS.map((hops) => (
          <option key={hops} value={hops}>
            {hops}
          </option>
        ))}
      </select>
      <label htmlFor={`step-${index}-direction`}>direction</label>
      <DirectionSelect
        id={`step-${index}-direction`}
        value={step.direction}
        onChange={(direction) => onChange({ ...step, direction })}
      />
      <label>
        edge types{" "}
        <input
          id={`step-${index}-types`}
          list="edge-types"
          value={step.ty_names}
          placeholder="any"
          onChange={(event) =>
            onChange({ ...step, ty_names: event.target.value })
          }
        />
      </label>
      <label htmlFor={`step-${index}-currentness`}>followed edges</label>
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

export function ExpandFieldset({ form, onChange }: FormProps) {
  const setStep = (index: number, step: ExpansionForm) =>
    onChange({
      ...form,
      expand: form.expand.map((existing, i) => (i === index ? step : existing)),
    });
  return (
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
        disabled={form.expand.length >= MAX_STEPS}
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
  );
}

export function EdgesFieldset({ form, onChange }: FormProps) {
  return (
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
        <label htmlFor="edges-currentness">result edges</label>
        <CurrentnessSelect
          id="edges-currentness"
          value={form.edges.currentness}
          onChange={(currentness) =>
            onChange({ ...form, edges: { ...form.edges, currentness } })
          }
        />
      </div>
    </fieldset>
  );
}

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

// The suggestion list the edge type inputs share.
export function EdgeTypeList() {
  const types = useTypes();
  return (
    <datalist id="edge-types">
      {edgeTypeNames(types).map((name) => (
        <option key={name} value={name} />
      ))}
    </datalist>
  );
}
