import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import { useData } from "../data.tsx";
import { DraftActions } from "./actions.ts";
import { type DraftState, initialDraft } from "./model.ts";
import {
  loadDraft,
  restoredState,
  saveDraft,
  storedChanged,
  storedOf,
} from "./persist.ts";
import { type DraftAction, draftReducer } from "./reducer.ts";

export interface Draft {
  state: DraftState;
  dispatch: (action: DraftAction) => void;
  actions: DraftActions;
}

export const DraftContext = createContext<Draft | null>(null);

export function useDraft(): Draft {
  const draft = useContext(DraftContext);
  if (draft === null) {
    throw new Error("useDraft needs a DraftProvider");
  }
  return draft;
}

// Holds the draft of one database. Mount it keyed by the database id: the
// stored draft is read once, on mount, and the stored parts are written on
// every dispatch that changes them, synchronously, so the marker of an apply
// is on disk before its request leaves; nothing is stored before the
// database is known.
export function DraftProvider({
  database,
  children,
}: {
  database: string | null;
  children: ReactNode;
}) {
  const { client, bumpRefresh } = useData();
  // The ref is the reducer's state and every dispatch advances it at once,
  // so an action that dispatches and then reads, such as adding the first
  // operation and then minting a plan ID, sees its own change; React state
  // only mirrors it for rendering. Advancing it during render instead would
  // lag one render behind the handlers.
  const current = useRef<DraftState | null>(null);
  const [state, setState] = useState(() => {
    const initial =
      database === null
        ? initialDraft
        : restoredState(loadDraft(window.localStorage, database));
    current.current = initial;
    return initial;
  });
  const dispatch = useCallback(
    (action: DraftAction) => {
      const previous = current.current ?? initialDraft;
      const next = draftReducer(previous, action);
      current.current = next;
      if (database !== null && storedChanged(previous, next)) {
        saveDraft(window.localStorage, database, storedOf(next));
      }
      setState(next);
    },
    [database],
  );
  const actions = useMemo(
    () =>
      new DraftActions(
        client,
        dispatch,
        () => current.current ?? initialDraft,
        bumpRefresh,
      ),
    [client, dispatch, bumpRefresh],
  );
  const value = useMemo(
    () => ({ state, dispatch, actions }),
    [state, dispatch, actions],
  );
  return <DraftContext value={value}>{children}</DraftContext>;
}
