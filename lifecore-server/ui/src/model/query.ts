import type {
  Currentness,
  Direction,
  EdgeFilter,
  EntryId,
  Expansion,
  Seed,
  ViewQueryParams,
} from "../contract.ts";
import { sameId } from "./labels.ts";

// The Explore form. Edge type names are kept as the typed text and split when
// the query is built, so the input never fights the user.
export interface NodeSeedForm {
  enabled: boolean;
  ty_name: string;
  text: string;
  currentness: Currentness;
}
export interface ExpansionForm {
  hops: number;
  direction: Direction;
  ty_names: string;
  currentness: Currentness;
}
export interface EdgeFilterForm {
  ty_names: string;
  currentness: Currentness;
}
export interface ExploreForm {
  ids: EntryId[];
  nodes: NodeSeedForm;
  expand: ExpansionForm[];
  edges: EdgeFilterForm;
}

// Client-side bounds on a query (INTERFACES §5.2); the server imposes none.
export const MAX_STEPS = 3;
export const MAX_HOPS = 3;

export function defaultExpansion(): ExpansionForm {
  return { hops: 1, direction: "both", ty_names: "", currentness: "current" };
}

export const emptyForm: ExploreForm = {
  ids: [],
  nodes: { enabled: false, ty_name: "", text: "", currentness: "current" },
  expand: [defaultExpansion()],
  edges: { ty_names: "", currentness: "current" },
};

// The Explore screen's state: the form, and the query last run with a counter
// so that running an unchanged form sends the request again.
export interface ExploreState {
  form: ExploreForm;
  submitted: { params: ViewQueryParams; run: number } | null;
}

export type ExploreAction =
  | { kind: "change"; form: ExploreForm }
  | { kind: "run" }
  // "Explore from here" and "+ seed": the node joins the seeds and the
  // query runs.
  | { kind: "seed"; id: EntryId }
  // "focus": the node becomes the only seed, the node selection included,
  // and the query runs with the same expansion.
  | { kind: "focus"; id: EntryId }
  // A saved view: its form replaces this one and runs.
  | { kind: "load"; form: ExploreForm };

export const initialExplore: ExploreState = {
  form: emptyForm,
  submitted: null,
};

export function exploreReducer(
  state: ExploreState,
  action: ExploreAction,
): ExploreState {
  switch (action.kind) {
    case "change":
      return { ...state, form: action.form };
    case "run":
      return run(state);
    case "seed":
      return run({ ...state, form: addSeed(state.form, action.id) });
    case "focus":
      return run({
        ...state,
        form: {
          ...state.form,
          ids: [action.id],
          nodes: { ...state.form.nodes, enabled: false },
        },
      });
    case "load":
      return run({ ...state, form: action.form });
  }
}

// A form without a seed has nothing to run and leaves the last result alone.
function run(state: ExploreState): ExploreState {
  const params = toParams(state.form);
  if (params === null) {
    return state;
  }
  return {
    ...state,
    submitted: { params, run: (state.submitted?.run ?? 0) + 1 },
  };
}

// Whether the form would send a different query than the one on show.
export function formChanged(state: ExploreState): boolean {
  if (state.submitted === null) {
    return false;
  }
  return (
    JSON.stringify(toParams(state.form)) !==
    JSON.stringify(state.submitted.params)
  );
}

export function addSeed(form: ExploreForm, id: EntryId): ExploreForm {
  if (form.ids.some((existing) => sameId(existing, id))) {
    return form;
  }
  return { ...form, ids: [...form.ids, id] };
}

export function removeSeed(form: ExploreForm, id: EntryId): ExploreForm {
  return { ...form, ids: form.ids.filter((existing) => !sameId(existing, id)) };
}

// Comma- or whitespace-separated names; none means any.
export function parseNames(text: string): string[] {
  return text.split(/[\s,]+/).filter((name) => name !== "");
}

// The request for the form, or null when it has no seed.
export function toParams(form: ExploreForm): ViewQueryParams | null {
  const seeds: Seed[] = [];
  if (form.ids.length > 0) {
    seeds.push({ kind: "ids", ids: form.ids });
  }
  if (form.nodes.enabled) {
    const seed: Extract<Seed, { kind: "nodes" }> = {
      kind: "nodes",
      currentness: form.nodes.currentness,
    };
    const ty_name = form.nodes.ty_name.trim();
    if (ty_name !== "") {
      seed.ty_name = ty_name;
    }
    const text = form.nodes.text.trim();
    if (text !== "") {
      seed.text = text;
    }
    seeds.push(seed);
  }
  if (seeds.length === 0) {
    return null;
  }
  const expand = form.expand.map((step) => {
    const expansion: Expansion = {
      hops: step.hops,
      direction: step.direction,
      currentness: step.currentness,
    };
    const ty_names = parseNames(step.ty_names);
    if (ty_names.length > 0) {
      expansion.ty_names = ty_names;
    }
    return expansion;
  });
  const edges: EdgeFilter = { currentness: form.edges.currentness };
  const edgeNames = parseNames(form.edges.ty_names);
  if (edgeNames.length > 0) {
    edges.ty_names = edgeNames;
  }
  return { seeds, expand, edges };
}

const CURRENTNESS: readonly Currentness[] = ["current", "historical", "all"];
const DIRECTIONS: readonly Direction[] = ["both", "outgoing", "incoming"];
const POSITION = /^(0|[1-9][0-9]*)$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCurrentness(value: unknown): value is Currentness {
  return CURRENTNESS.includes(value as Currentness);
}

export function readEntryId(value: unknown): EntryId | null {
  if (
    isRecord(value) &&
    typeof value.origin === "string" &&
    typeof value.position === "string" &&
    POSITION.test(value.position)
  ) {
    return { origin: value.origin, position: value.position };
  }
  return null;
}

function readExpansion(value: unknown): ExpansionForm | null {
  if (
    isRecord(value) &&
    Number.isInteger(value.hops) &&
    (value.hops as number) >= 1 &&
    (value.hops as number) <= MAX_HOPS &&
    DIRECTIONS.includes(value.direction as Direction) &&
    typeof value.ty_names === "string" &&
    isCurrentness(value.currentness)
  ) {
    return {
      hops: value.hops as number,
      direction: value.direction as Direction,
      ty_names: value.ty_names,
      currentness: value.currentness,
    };
  }
  return null;
}

// A stored form, as saved views keep it, or null when anything in it is not
// a form this client would build.
export function readForm(value: unknown): ExploreForm | null {
  if (
    !isRecord(value) ||
    !Array.isArray(value.ids) ||
    !isRecord(value.nodes) ||
    !Array.isArray(value.expand) ||
    !isRecord(value.edges)
  ) {
    return null;
  }
  const ids = value.ids.map(readEntryId);
  const expand = value.expand.map(readExpansion);
  const { nodes, edges } = value;
  if (
    ids.some((id) => id === null) ||
    expand.some((step) => step === null) ||
    expand.length === 0 ||
    expand.length > MAX_STEPS ||
    typeof nodes.enabled !== "boolean" ||
    typeof nodes.ty_name !== "string" ||
    typeof nodes.text !== "string" ||
    !isCurrentness(nodes.currentness) ||
    typeof edges.ty_names !== "string" ||
    !isCurrentness(edges.currentness)
  ) {
    return null;
  }
  return {
    ids: ids.filter((id) => id !== null),
    nodes: {
      enabled: nodes.enabled,
      ty_name: nodes.ty_name,
      text: nodes.text,
      currentness: nodes.currentness,
    },
    expand: expand.filter((step) => step !== null),
    edges: { ty_names: edges.ty_names, currentness: edges.currentness },
  };
}

// One line for a folded form: its seeds, then its steps.
export function querySummary(form: ExploreForm): string {
  const seeds = form.ids.map((id) => `@${id.position}`);
  if (form.nodes.enabled) {
    const text = form.nodes.text.trim();
    seeds.push(
      `${form.nodes.currentness} ${form.nodes.ty_name.trim() || "any type"}${
        text === "" ? "" : ` containing "${text}"`
      }`,
    );
  }
  const steps = form.expand.map((step) => {
    const names = parseNames(step.ty_names);
    return `${step.hops} hop${step.hops === 1 ? "" : "s"} ${step.direction}${
      names.length === 0 ? "" : ` over ${names.join(", ")}`
    }`;
  });
  return `${seeds.length === 0 ? "no seeds" : seeds.join(" + ")}, then ${steps.join(", then ")}`;
}
