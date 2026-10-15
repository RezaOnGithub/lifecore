import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useData } from "../data.tsx";
import { DraftActions } from "./actions.ts";
import {
  type DraftAction,
  type DraftState,
  draftReducer,
  initialDraft,
} from "./model.ts";
import { loadDraft, saveDraft } from "./persist.ts";

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

function restored(database: string | null): DraftState {
  if (database === null) {
    return initialDraft;
  }
  const proposal = loadDraft(window.localStorage, database);
  return proposal === null ? initialDraft : { ...initialDraft, proposal };
}

// Holds the draft of one database. Mount it keyed by the database id: the
// stored draft is read once, on mount, and written on every change of the
// proposal; nothing is stored before the database is known.
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
    const initial = restored(database);
    current.current = initial;
    return initial;
  });
  const dispatch = useCallback((action: DraftAction) => {
    const next = draftReducer(current.current ?? initialDraft, action);
    current.current = next;
    setState(next);
  }, []);
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
  useEffect(() => {
    if (database !== null) {
      saveDraft(window.localStorage, database, state.proposal);
    }
  }, [database, state.proposal]);
  const value = useMemo(
    () => ({ state, dispatch, actions }),
    [state, dispatch, actions],
  );
  return <DraftContext value={value}>{children}</DraftContext>;
}
