import type { Currentness, EntryId } from "./contract.ts";
import type { Edit, Open } from "./links.tsx";
import type { Sort } from "./model/columns.ts";
import { sameId } from "./model/labels.ts";
import { emptyOutline, type OutlineView } from "./model/outline.ts";
import {
  type ExploreForm,
  type ExploreState,
  type ExploreStep,
  emptyForm,
  toParams,
} from "./model/query.ts";
import type { ParentRelation } from "./model/thread.ts";

// Rows a table renders before "show next" and "show all".
export const ROW_PAGE = 500;

// What a table of entries remembers: its sort, the relationship cells
// whose neighbour list is open (`<row key>|<column key>`), and how many
// rows it renders, null for all of them.
export interface TableView {
  sort: Sort | null;
  open: readonly string[];
  limit: number | null;
}

export const defaultTableView: TableView = {
  sort: null,
  open: [],
  limit: ROW_PAGE,
};

export interface TableScreenView {
  currentness: Currentness;
  text: string;
  relationships: boolean;
  table: TableView;
}

export const defaultTableScreenView: TableScreenView = {
  currentness: "current",
  text: "",
  relationships: true,
  table: defaultTableView,
};

export interface InspectorView {
  outline: OutlineView;
  raw: boolean;
}

export const defaultInspectorView: InspectorView = {
  outline: emptyOutline,
  raw: false,
};

export type Display = "table" | "thread" | "graph";

export interface ExploreView {
  display: Display;
  relationships: boolean;
  unfolded: boolean;
  // The thread's parent relation, null for its default.
  thread: ParentRelation | null;
  // Each type's table in the results.
  tables: Readonly<Record<string, TableView>>;
}

export const defaultExploreView: ExploreView = {
  display: "table",
  relationships: true,
  unfolded: true,
  thread: null,
  tables: {},
};

// The planning valuation's screen remembers the deadline sample as typed,
// empty for the server's default.
export interface ValuationView {
  windows: string;
}

export const defaultValuationView: ValuationView = { windows: "" };

export type ExploreScreen = {
  kind: "explore";
  view: ExploreView;
} & ExploreState;

export type Screen =
  | { kind: "database" }
  | { kind: "table"; ty_name: string; view?: TableScreenView }
  | { kind: "inspector"; id: EntryId; view?: InspectorView }
  | ExploreScreen
  | { kind: "findings" }
  | { kind: "valuation"; view?: ValuationView }
  // A draft operation, by its name; a renamed one is a different screen.
  | { kind: "operation"; name: string }
  | { kind: "import" };

export const initialExplore: ExploreScreen = {
  kind: "explore",
  form: emptyForm,
  ran: null,
  view: defaultExploreView,
};

// One place in the Back history: a screen with what it remembers, and the
// main pane's scroll offset when it was left.
export interface Step {
  id: number;
  screen: Screen;
  scroll: number;
}

// In-memory navigation: the current step and the stack Back pops. There is
// no URL routing in the prototype. `explore` is the last Explore screen on
// show, which stays mounted under the others and which the sidebar reopens.
export interface Navigation {
  current: Step;
  history: Step[];
  next: number;
  explore: ExploreScreen;
}

export type NavigationAction =
  // `scroll` is the offset of the step being left; `leaving`, when given,
  // replaces its screen first, as when a run leaves the previous query
  // behind as it was run.
  | { kind: "open"; screen: Screen; scroll: number; leaving?: Screen }
  // Swaps the current screen without a step, as when an operation is renamed
  // under its editor.
  | { kind: "replace"; screen: Screen }
  // Changes what the current step remembers; ignored once that step is no
  // longer current.
  | { kind: "edit"; step: number; edit: (screen: Screen) => Screen }
  // Back one step, or to the history entry at `to`.
  | { kind: "back"; to?: number };

export const initialNavigation: Navigation = {
  current: { id: 0, screen: { kind: "database" }, scroll: 0 },
  history: [],
  next: 1,
  explore: initialExplore,
};

// The query an Explore screen shows, with its run counter: two screens
// showing the same run are one place.
function ranKey(screen: ExploreScreen): string {
  return JSON.stringify(
    screen.ran === null
      ? null
      : { params: toParams(screen.ran.form), run: screen.ran.run },
  );
}

export function sameScreen(a: Screen, b: Screen): boolean {
  switch (a.kind) {
    case "table":
      return b.kind === "table" && a.ty_name === b.ty_name;
    case "inspector":
      return b.kind === "inspector" && sameId(a.id, b.id);
    case "operation":
      return b.kind === "operation" && a.name === b.name;
    case "explore":
      return b.kind === "explore" && ranKey(a) === ranKey(b);
    default:
      return a.kind === b.kind;
  }
}

function withExplore(state: Navigation): Navigation {
  const { screen } = state.current;
  return screen.kind === "explore" && screen !== state.explore
    ? { ...state, explore: screen }
    : state;
}

export function navigate(
  state: Navigation,
  action: NavigationAction,
): Navigation {
  switch (action.kind) {
    case "open": {
      const left: Step = {
        ...state.current,
        screen: action.leaving ?? state.current.screen,
        scroll: action.scroll,
      };
      // Reopening the current screen is not a step, and keeps what it
      // remembers; Back would otherwise appear to do nothing.
      if (sameScreen(state.current.screen, action.screen)) {
        return state;
      }
      return withExplore({
        ...state,
        current: { id: state.next, screen: action.screen, scroll: 0 },
        history: [...state.history, left],
        next: state.next + 1,
      });
    }
    case "replace":
      return withExplore({
        ...state,
        current: { ...state.current, screen: action.screen },
      });
    case "edit": {
      if (action.step !== state.current.id) {
        return state;
      }
      const screen = action.edit(state.current.screen);
      return screen === state.current.screen
        ? state
        : withExplore({ ...state, current: { ...state.current, screen } });
    }
    case "back": {
      const to = action.to ?? state.history.length - 1;
      const previous = state.history[to];
      if (previous === undefined) {
        return state;
      }
      return withExplore({
        ...state,
        current: previous,
        history: state.history.slice(0, to),
      });
    }
  }
}

// An Explore screen whose form runs on arrival.
export function exploreScreen(
  form: ExploreForm,
  view: ExploreView = defaultExploreView,
): ExploreScreen {
  return { kind: "explore", form, ran: { form, run: 0 }, view };
}

// Puts an Explore step into the history: a new query is a new step, and the
// step it leaves shows its own query again.
export function applyExploreStep(
  step: ExploreStep,
  view: ExploreView,
  open: Open,
  edit: Edit,
) {
  switch (step.kind) {
    case "none":
      return;
    case "replace":
      edit((screen) =>
        screen.kind === "explore" ? { ...screen, ...step.state } : screen,
      );
      return;
    case "push":
      open(
        { kind: "explore", ...step.state, view },
        { leaving: { kind: "explore", ...step.leaving, view } },
      );
  }
}

const INSPECT_HREF = /^#inspect\/([^/]+)\/(0|[1-9][0-9]*)$/;

// The entry a rendered graph's link names: `views.render` gives every node
// and edge an `href` of `#inspect/<origin>/<position>`. Null for any other
// link.
export function inspectHref(href: string | null): EntryId | null {
  const match = href === null ? null : INSPECT_HREF.exec(href);
  if (match === null) {
    return null;
  }
  const [, origin, position] = match;
  return origin === undefined || position === undefined
    ? null
    : { origin, position };
}
