import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { useData } from "../data.tsx";
import type { Alias, AppliedName } from "../model/aliases.ts";
import { DraftActions } from "./actions.ts";
import { applyingElsewhere, type DraftState } from "./model.ts";
import { storageKey } from "./persist.ts";
import type { DraftAction } from "./reducer.ts";
import { SharedDraft } from "./shared.ts";

export interface Draft {
  state: DraftState;
  dispatch: (action: DraftAction) => void;
  actions: DraftActions;
}

export const DraftContext = createContext<Draft | null>(null);

const noAliases = () => [];
const ignoreApplied = () => {};

export function useDraft(): Draft {
  const draft = useContext(DraftContext);
  if (draft === null) {
    throw new Error("useDraft needs a DraftProvider");
  }
  return draft;
}

// Holds the draft of one database. Mount it keyed by the database id: the
// stored draft is read on mount, the stored parts are written on every
// dispatch that changes them, and another tab's write is followed through
// the `storage` event (see `SharedDraft`). Before the database is known the
// draft is detached: nothing is stored and no edit lands. `aliases` reads the
// names kept for entries, and `onApplied` receives what an apply means for
// them; both must keep their identity.
export function DraftProvider({
  database,
  aliases = noAliases,
  onApplied = ignoreApplied,
  children,
}: {
  database: string | null;
  aliases?: () => readonly Alias[];
  onApplied?: (names: readonly AppliedName[]) => void;
  children: ReactNode;
}) {
  const { client, bumpRefresh } = useData();
  // The shared draft is the reducer's state and every dispatch advances it
  // at once, so an action that dispatches and then reads, such as adding
  // the first operation and then minting a plan ID, sees its own change;
  // React state only mirrors it for rendering.
  const [shared] = useState(
    () => new SharedDraft(window.localStorage, database),
  );
  const [state, setState] = useState<DraftState>(shared.state);
  // Only the tab that made an applied record hands its names on, once, at
  // the dispatch that made it: the record is never stored, so another tab
  // or a reload cannot hand them on again over a later change.
  const dispatch = useCallback(
    (action: DraftAction) => {
      const before = shared.state.applied;
      const after = shared.dispatch(action);
      setState(after);
      if (after.applied !== null && after.applied !== before) {
        onApplied(after.applied.names);
      }
    },
    [shared, onApplied],
  );
  useEffect(() => {
    if (database === null) {
      return;
    }
    const onStorage = (event: StorageEvent) => {
      // A null key is a `clear()` of the whole storage.
      if (
        event.storageArea !== window.localStorage ||
        (event.key !== null && event.key !== storageKey(database))
      ) {
        return;
      }
      const before = shared.state;
      if (shared.sync()) {
        setState(shared.state);
        // Another tab's apply ended and may have been published.
        if (applyingElsewhere(before) && !applyingElsewhere(shared.state)) {
          bumpRefresh();
        }
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [shared, database, bumpRefresh]);
  const actions = useMemo(
    () =>
      new DraftActions(
        client,
        dispatch,
        () => shared.state,
        bumpRefresh,
        aliases,
      ),
    [client, dispatch, shared, bumpRefresh, aliases],
  );
  const value = useMemo(
    () => ({ state, dispatch, actions }),
    [state, dispatch, actions],
  );
  return <DraftContext value={value}>{children}</DraftContext>;
}
