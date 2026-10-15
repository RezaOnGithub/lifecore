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
  // "Explore from here": the node joins the seeds and the query runs.
  | { kind: "seed"; id: EntryId };

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
