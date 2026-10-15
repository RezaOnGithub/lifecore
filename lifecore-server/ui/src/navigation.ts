import type { EntryId } from "./contract.ts";
import { sameId } from "./model/labels.ts";

export type Screen =
  | { kind: "database" }
  | { kind: "table"; ty_name: string }
  | { kind: "inspector"; id: EntryId }
  | { kind: "explore" };

// In-memory navigation: the current screen and the stack Back pops. There is
// no URL routing in the prototype.
export interface Navigation {
  current: Screen;
  history: Screen[];
}

export type NavigationAction =
  | { kind: "open"; screen: Screen }
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
    case "back": {
      const previous = state.history.at(-1);
      if (previous === undefined) {
        return state;
      }
      return { current: previous, history: state.history.slice(0, -1) };
    }
  }
}
