import type {
  AddAnalyzerParams,
  AnalyzerAdded,
  AnalyzerConfig,
  EntryId,
  ErrorKind,
  EvaluatedView,
  ExecutionFailure,
  NodeRecord,
  ProposalReport,
  Uuid,
} from "../contract.ts";
import type { Counts } from "../draft/findings.ts";
import type { Evaluation } from "../draft/model.ts";
import {
  type Call,
  HttpRejectionError,
  RpcError,
  TransportError,
} from "../rpc.ts";
import { readLabels } from "./entryLabels.ts";

// The Add analyzer form, kept across visits of the Database screen. `name`
// is null until the user picks one, for the screen's default.
export interface AddForm {
  open: boolean;
  name: string | null;
  configuration: string;
  evaluation: Evaluation;
}

// One confirmed request: its exact parameters, and the database and suite
// on show when it was confirmed.
export interface Sent {
  database: Uuid;
  params: AddAnalyzerParams & { configuration: string };
  suite: AnalyzerConfig[];
}

// What a failed `analyzers.add` says about the suite.
export type Classified =
  | { kind: "rejected"; report: ProposalReport }
  | {
      kind: "analyzer_failure";
      message: string;
      failure: ExecutionFailure | null;
    }
  | { kind: "busy"; message: string }
  | { kind: "failed"; failure: unknown }
  | {
      kind: "uncertain";
      error: string;
      message: string;
      outcome: AnalyzerAdded | null;
    };

// The suite read again after an add of unknown fate.
export type Reread =
  | { status: "reading" }
  | { status: "read"; suite: AnalyzerConfig[] }
  | { status: "failed"; failure: unknown };

// A rejection's labels arrive after it; an uncertain add keeps its re-read.
export type AddOutcome =
  | { kind: "sending"; sent: Sent }
  | { kind: "added"; sent: Sent; added: AnalyzerAdded }
  | (Extract<Classified, { kind: "rejected" }> & {
      sent: Sent;
      nodes?: Map<string, NodeRecord>;
    })
  | (Extract<Classified, { kind: "uncertain" }> & {
      sent: Sent;
      reread?: Reread;
    })
  | (Exclude<Classified, { kind: "rejected" | "uncertain" }> & { sent: Sent });

// The kinds `analyzers.add` reports before it publishes anything. Every
// other failure, unknown kinds included, may follow a publication, as for a
// draft's apply (`draft/classify.ts`).
const BEFORE_PUBLICATION: ReadonlySet<ErrorKind> = new Set<ErrorKind>([
  "invalid_params",
  "local_time_unknown",
  "rejected",
  "analyzer_failure",
  "busy",
  "lock_failed",
  "database_missing",
  "database_unreadable",
  "publication_failed",
]);

export function classifyAdd(failure: unknown): Classified {
  const text = failure instanceof Error ? failure.message : String(failure);
  if (failure instanceof RpcError) {
    const { error } = failure;
    // An envelope error is detected before dispatch.
    if (!("data" in error)) {
      return { kind: "failed", failure };
    }
    const { data } = error;
    if (!BEFORE_PUBLICATION.has(data.kind)) {
      let outcome: AnalyzerAdded | null = null;
      if (
        data.kind === "publication_uncertain" &&
        data.outcome.kind === "analyzer_added"
      ) {
        const { graph_revision, suite, report } = data.outcome;
        outcome = { graph_revision, suite, report };
      }
      return {
        kind: "uncertain",
        error: data.kind,
        message: error.message,
        outcome,
      };
    }
    switch (data.kind) {
      case "rejected":
        return { kind: "rejected", report: data.report };
      case "analyzer_failure":
        return {
          kind: "analyzer_failure",
          message: error.message,
          failure: data.failure ?? null,
        };
      case "busy":
        return { kind: "busy", message: error.message };
      default:
        return { kind: "failed", failure };
    }
  }
  // A structured rejection comes from the access checks, before dispatch;
  // any other refused answer is not known to.
  if (failure instanceof HttpRejectionError) {
    return failure.rejection === null
      ? {
          kind: "uncertain",
          error: `http_${failure.status}`,
          message: text,
          outcome: null,
        }
      : { kind: "failed", failure };
  }
  // A lost answer, or one that could not be read.
  return {
    kind: "uncertain",
    error: failure instanceof TransportError ? "transport" : "unreadable",
    message: text,
    outcome: null,
  };
}

function sameConfig(a: AnalyzerConfig, b: AnalyzerConfig): boolean {
  return a.name === b.name && a.configuration === b.configuration;
}

// The one-based suite positions holding the analyzer the request names.
export function positionsOf(
  suite: readonly AnalyzerConfig[],
  sent: Sent,
): number[] {
  const wanted = {
    name: sent.params.name,
    configuration: sent.params.configuration,
  };
  return suite.flatMap((config, index) =>
    sameConfig(config, wanted) ? [index + 1] : [],
  );
}

// Whether a suite read after an add of unknown fate holds one more copy of
// its analyzer than the suite it was confirmed against: an identical one
// may already have been there, and another writer may append in between,
// so the count is compared, not the last position.
export function suiteHolds(
  suite: readonly AnalyzerConfig[],
  sent: Sent,
): boolean {
  return positionsOf(suite, sent).length > positionsOf(sent.suite, sent).length;
}

// Findings of a report by category, all of them and those of the analyzer
// the request named.
export function addCounts(
  report: ProposalReport,
  sent: Sent,
): { total: Counts; added: Counts } {
  const total: Counts = { inconsistency: 0, warning: 0, suggestion: 0 };
  const added: Counts = { inconsistency: 0, warning: 0, suggestion: 0 };
  for (const { key, diagnostic } of report.analysis.observations) {
    total[diagnostic.kind] += 1;
    if (
      key.analyzer.name === sent.params.name &&
      key.analyzer.configuration === sent.params.configuration
    ) {
      added[diagnostic.kind] += 1;
    }
  }
  return { total, added };
}

export function boundaryWords(view: EvaluatedView): string {
  if (view.kind === "initialization") {
    return "at its initialization";
  }
  const { first, last } = view;
  return first.end_exclusive === last.end_exclusive
    ? `at the prefix boundary ${first.end_exclusive}`
    : `at the prefix boundaries ${first.end_exclusive}–${last.end_exclusive}`;
}

function inconsistencyIds(report: ProposalReport): EntryId[] {
  return report.analysis.observations.flatMap(({ diagnostic }) =>
    diagnostic.kind !== "inconsistency"
      ? []
      : [
          ...(diagnostic.focus === null ? [] : [diagnostic.focus]),
          ...diagnostic.subjects,
        ],
  );
}

// The Database screen's add form and its last request, per page client, so
// both survive a detour through the inspector and an answer that arrives
// after the screen unmounted still lands. A confirmed request is sent once
// and never retried; only the user sends another.
export class AnalyzerAdds {
  readonly #call: Call;
  readonly #listeners = new Set<() => void>();
  #version = 0;
  #form: AddForm = {
    open: false,
    name: null,
    configuration: "{}",
    evaluation: { kind: "none" },
  };
  #outcome: AddOutcome | null = null;

  constructor(call: Call) {
    this.#call = call;
  }

  subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  version = () => this.#version;

  #changed() {
    this.#version += 1;
    for (const listener of this.#listeners) {
      listener();
    }
  }

  form(): AddForm {
    return this.#form;
  }

  setForm(change: Partial<AddForm>) {
    this.#form = { ...this.#form, ...change };
    this.#changed();
  }

  outcome(): AddOutcome | null {
    return this.#outcome;
  }

  sending(): boolean {
    return this.#outcome?.kind === "sending";
  }

  // An add of unknown fate stays until the suite has been read again.
  dismiss() {
    const outcome = this.#outcome;
    if (
      outcome?.kind === "sending" ||
      (outcome?.kind === "uncertain" && outcome.reread?.status !== "read")
    ) {
      return;
    }
    this.#outcome = null;
    this.#changed();
  }

  // Whether Add must wait: a request is in flight, or one of unknown fate
  // may have appended the analyzer to a suite not read since, and a second
  // one could append it twice.
  waiting(): boolean {
    const outcome = this.#outcome;
    return (
      outcome?.kind === "sending" ||
      (outcome?.kind === "uncertain" && outcome.reread?.status !== "read")
    );
  }

  // Reads the suite again after an add of unknown fate, by itself rather
  // than through the screens' reload, so the read is known to follow the
  // failure.
  async reread(): Promise<void> {
    const outcome = this.#outcome;
    if (outcome?.kind !== "uncertain" || outcome.reread?.status === "reading") {
      return;
    }
    const reading = { ...outcome, reread: { status: "reading" } as Reread };
    this.#outcome = reading;
    this.#changed();
    let reread: Reread;
    try {
      const { suite } = await this.#call("database.describe", {});
      reread = { status: "read", suite };
    } catch (failure) {
      reread = { status: "failed", failure };
    }
    if (this.#outcome === reading) {
      this.#outcome = { ...outcome, reread };
      this.#changed();
    }
  }

  // Sends `sent` once, unless a request is in flight. `onAdded` runs after
  // a success, also when the screen has gone; a rejection's inconsistencies
  // then get their labels, read at the report's boundary.
  async add(sent: Sent, onAdded: () => void): Promise<void> {
    if (this.waiting()) {
      return;
    }
    const sending: AddOutcome = { kind: "sending", sent };
    this.#outcome = sending;
    this.#changed();
    let classified: Classified;
    try {
      const added = await this.#call("analyzers.add", sent.params);
      this.#outcome = { kind: "added", sent, added };
      this.#changed();
      onAdded();
      return;
    } catch (failure) {
      classified = classifyAdd(failure);
    }
    if (classified.kind !== "rejected") {
      this.#outcome = { ...classified, sent };
      this.#changed();
      return;
    }
    const settled = { ...classified, sent };
    this.#outcome = settled;
    this.#changed();
    const { report } = classified;
    const { nodes } = await readLabels(
      this.#call,
      inconsistencyIds(report),
      report.analysis.view.end_exclusive,
    );
    if (this.#outcome === settled) {
      this.#outcome = { ...settled, nodes };
      this.#changed();
    }
  }
}
