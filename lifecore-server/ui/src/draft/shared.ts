import { type DraftState, detachedDraft } from "./model.ts";
import {
  parseStored,
  restoredState,
  type Store,
  saveGuarded,
  storedChanged,
  storedText,
} from "./persist.ts";
import { type DraftAction, draftReducer } from "./reducer.ts";

// The draft of one database as one tab holds it, kept in step with the
// stored record that every tab of that database shares. `#known` is the text
// this tab last wrote or read, so a different stored text is another tab's
// write. Without a database nothing is read or stored, and the draft stays
// detached, which freezes it.
export class SharedDraft {
  readonly #store: Store;
  readonly #database: string | null;
  #known: string | null | undefined = null;
  #state: DraftState = detachedDraft;

  constructor(store: Store, database: string | null) {
    this.#store = store;
    this.#database = database;
    if (database !== null) {
      const text = storedText(store, database);
      this.#known = text;
      this.#state = restoredState(parseStored(text));
    }
  }

  get state(): DraftState {
    return this.#state;
  }

  // Applies `action` and saves the stored parts when they changed,
  // synchronously, so an apply's marker is stored before its request
  // leaves. When another tab's marker is stored instead, the change is
  // dropped and that tab's record adopted.
  dispatch(action: DraftAction): DraftState {
    const previous = this.#state;
    let next = draftReducer(previous, action);
    if (this.#database !== null && storedChanged(previous, next)) {
      const saved = saveGuarded(
        this.#store,
        this.#database,
        this.#known,
        previous,
        next,
      );
      if (saved.kind === "refused") {
        this.#known = saved.text;
        next = draftReducer(previous, {
          kind: "adopt",
          stored: saved.stored,
          refused: true,
        });
      } else if (saved.kind === "written") {
        this.#known = saved.text;
      }
    }
    this.#state = next;
    return next;
  }

  // Follows another tab's write, reading the key again rather than trusting
  // the event, which may be older than this tab's own last write. While this
  // tab's apply is in flight it adopts nothing: it writes its own record
  // again over one without markers, so a write that raced past the marker
  // cannot erase it. True when the state changed.
  sync(): boolean {
    if (this.#database === null) {
      return false;
    }
    const text = storedText(this.#store, this.#database);
    if (text === undefined || text === this.#known) {
      return false;
    }
    const state = this.#state;
    if (state.pending === "apply") {
      const saved = saveGuarded(
        this.#store,
        this.#database,
        this.#known,
        state,
        state,
      );
      if (saved.kind === "written") {
        this.#known = saved.text;
      }
      return false;
    }
    this.#known = text;
    const stored = parseStored(text);
    // An unreadable record is replaced by this tab's next save.
    if (text !== null && stored === null) {
      return false;
    }
    this.#state = draftReducer(state, { kind: "adopt", stored });
    return true;
  }
}
