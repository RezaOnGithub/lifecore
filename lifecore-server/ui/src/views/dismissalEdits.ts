import type { Dismissal, ErrorKind } from "../contract.ts";
import { HttpRejectionError, RpcError } from "../rpc.ts";

// A failed dismissal edit, by what it says about the stored records. Only
// failures known to come before publication leave them as they were;
// `written` is a publication whose durability is unconfirmed, with the
// records it published.
export type EditFailure =
  | { kind: "failed"; failure: unknown }
  | { kind: "written"; failure: unknown; dismissals: Dismissal[] }
  | { kind: "uncertain"; failure: unknown };

const BEFORE_PUBLICATION: ReadonlySet<ErrorKind> = new Set<ErrorKind>([
  "busy",
  "lock_failed",
  "publication_failed",
  "database_missing",
  "database_unreadable",
  "invalid_params",
  "foreign_database",
  "cannot_dismiss_inconsistency",
]);

// Mirrors the draft's apply classification (`draft/classify.ts`): envelope
// errors and structured HTTP rejections precede dispatch, and every other
// failure, unknown kinds and lost answers included, may follow a write.
export function classifyEdit(failure: unknown): EditFailure {
  if (failure instanceof RpcError) {
    const { error } = failure;
    if (!("data" in error) || BEFORE_PUBLICATION.has(error.data.kind)) {
      return { kind: "failed", failure };
    }
    const { data } = error;
    if (
      data.kind === "publication_uncertain" &&
      (data.outcome.kind === "dismissed" || data.outcome.kind === "restored")
    ) {
      return { kind: "written", failure, dismissals: data.outcome.dismissals };
    }
    return { kind: "uncertain", failure };
  }
  if (failure instanceof HttpRejectionError && failure.rejection !== null) {
    return { kind: "failed", failure };
  }
  return { kind: "uncertain", failure };
}

// A single edit is refused while another edit or a "Dismiss all" is under
// way.
export type EditResult = "stored" | EditFailure["kind"] | "refused";

// A "Dismiss all" over the keys of one cluster, one edit at a time.
export interface Bulk {
  cluster: string;
  label: string;
  total: number;
  stored: number;
  failed: number;
  running: boolean;
  stopping: boolean;
  // Why it ended before its last key, if it did.
  stopped: string | null;
}
