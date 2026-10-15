import type { EntryId } from "./contract.ts";
import { sameId } from "./model/labels.ts";

export type Screen =
  | { kind: "database" }
  | { kind: "table"; ty_name: string }
  | { kind: "inspector"; id: EntryId }
  | { kind: "explore" }
  // A draft operation, by its name; a renamed one is a different screen.
  | { kind: "operation"; name: string }
  | { kind: "import" };

// In-memory navigation: the current screen and the stack Back pops. There is
// no URL routing in the prototype.
export interface Navigation {
  current: Screen;
  history: Screen[];
}

export type NavigationAction =
  | { kind: "open"; screen: Screen }
  // Swaps the current screen without a step, as when an operation is renamed
  // under its editor.
  | { kind: "replace"; screen: Screen }
  | { kind: "back" };

export const initialNavigation: Navigation = {
  current: { kind: "database" },
  history: [],
};

export function sameScreen(a: Screen, b: Screen): boolean {
  switch (a.kind) {
    case "table":
      return b.kind === "table" && a.ty_name === b.ty_name;
    case "inspector":
      return b.kind === "inspector" && sameId(a.id, b.id);
    case "operation":
      return b.kind === "operation" && a.name === b.name;
    default:
      return a.kind === b.kind;
  }
}

export function navigate(
  state: Navigation,
  action: NavigationAction,
): Navigation {
  switch (action.kind) {
    case "open":
      // Reopening the current screen is not a step; Back would otherwise
      // appear to do nothing.
      if (sameScreen(state.current, action.screen)) {
        return state;
      }
      return {
        current: action.screen,
        history: [...state.history, state.current],
      };
    case "replace":
      return { ...state, current: action.screen };
    case "back": {
      const previous = state.history.at(-1);
      if (previous === undefined) {
        return state;
      }
      return { current: previous, history: state.history.slice(0, -1) };
    }
  }
}
