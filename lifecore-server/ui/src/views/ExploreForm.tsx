import type { Direction, EntryId, TypeSummary } from "../contract.ts";
import { useTypes } from "../data.tsx";
import { IdLink } from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey, typeName } from "../model/labels.ts";
import {
  defaultExpansion,
  type ExpansionForm,
  type ExploreForm,
  MAX_HOPS,
  MAX_STEPS,
  removeSeed,
} from "../model/query.ts";
import { CurrentnessSelect } from "./widgets.tsx";

interface FormProps {
  form: ExploreForm;
  onChange: (form: ExploreForm) => void;
}

export function SeedsFieldset({ form, onChange }: FormProps) {
  const types = useTypes();
  return (
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
              {typeName(type.name)}
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
