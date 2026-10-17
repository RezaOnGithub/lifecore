import type {
  AnalysisOutput,
  AnalyzerIdentity,
  DecimalString,
  Dismissal,
  EntryId,
  EvaluatedView,
  FindingKey,
  MessageType,
  Observation,
} from "../contract.ts";
import { sameId } from "./labels.ts";
import { counted } from "./words.ts";

// Report presentation of a stored-database analysis (SPEC §8.3), mirroring
// core `AnalysisOutput::groups`: findings are grouped by complete key and
// filtered for display; nothing here changes what the analysis found.

// The complete finding key as text, members in a fixed order, so equal keys
// give equal text however a response ordered them. The certificate is
// compared whole and never parsed.
export function keyText(key: FindingKey): string {
  return JSON.stringify([
    key.database,
    key.analyzer.name,
    key.analyzer.version,
    key.analyzer.configuration,
    key.kind,
    key.analyzer_certificate,
  ]);
}

export type Selection = "current" | "history";

// Inconsistencies are always on show, whatever the filter says.
export interface FindingFilter {
  selection: Selection;
  warning: boolean;
  suggestion: boolean;
  dismissed: boolean;
}

export const defaultFilter: FindingFilter = {
  selection: "current",
  warning: true,
  suggestion: true,
  dismissed: false,
};

export interface FindingGroup {
  key: FindingKey;
  id: string;
  // Every observation of the key, and those on show, in emission order.
  observations: Observation[];
  selected: Observation[];
  // Never set for an inconsistency, which cannot be dismissed.
  dismissal: Dismissal | null;
}

// Emitted at the boundary itself, or at the suite's initialization.
export function atBoundary(
  observation: Observation,
  output: AnalysisOutput,
): boolean {
  const { evaluated_view } = observation;
  return (
    evaluated_view.kind === "initialization" ||
    evaluated_view.last.end_exclusive === output.view.end_exclusive
  );
}

// A sticky advisory whose range ended before the boundary stays in current
// reports, but nothing says it still applies.
export function unconfirmed(
  observation: Observation,
  boundary: DecimalString,
): boolean {
  return (
    observation.diagnostic.sticky &&
    rangeWords(observation.evaluated_view, boundary).ended
  );
}

export function dismissalIndex(
  dismissals: readonly Dismissal[],
): Map<string, Dismissal> {
  const index = new Map<string, Dismissal>();
  for (const dismissal of dismissals) {
    const id = keyText(dismissal.key);
    // The first record of a key wins, as in core `Dismissals::get`.
    if (!index.has(id)) {
      index.set(id, dismissal);
    }
  }
  return index;
}

export function groupFindings(
  output: AnalysisOutput,
  dismissals: readonly Dismissal[],
  filter: FindingFilter,
): FindingGroup[] {
  const recorded = dismissalIndex(dismissals);
  const groups = new Map<string, FindingGroup>();
  for (const observation of output.observations) {
    const { key } = observation;
    const id = keyText(key);
    let group = groups.get(id);
    if (group === undefined) {
      group = {
        key,
        id,
        observations: [],
        selected: [],
        dismissal:
          key.kind === "inconsistency" ? null : (recorded.get(id) ?? null),
      };
      groups.set(id, group);
    }
    group.observations.push(observation);
    const kind = observation.diagnostic.kind;
    const category = kind === "inconsistency" || filter[kind];
    const boundary =
      filter.selection === "history" ||
      atBoundary(observation, output) ||
      observation.diagnostic.sticky;
    if (
      kind === "inconsistency" ||
      (category && boundary && (filter.dismissed || group.dismissal === null))
    ) {
      group.selected.push(observation);
    }
  }
  return [...groups.values()].filter((group) => group.selected.length > 0);
}

export type CategoryCounts = {
  [K in MessageType]: { findings: number; dismissed: number };
};

// Findings per category under a selection, before the category and
// dismissal filters, with how many of them are dismissed.
export function categoryCounts(
  output: AnalysisOutput,
  dismissals: readonly Dismissal[],
  selection: Selection,
): CategoryCounts {
  const counts: CategoryCounts = {
    inconsistency: { findings: 0, dismissed: 0 },
    warning: { findings: 0, dismissed: 0 },
    suggestion: { findings: 0, dismissed: 0 },
  };
  const all = { selection, warning: true, suggestion: true, dismissed: true };
  for (const group of groupFindings(output, dismissals, all)) {
    const count = counts[group.key.kind];
    count.findings += 1;
    if (group.dismissal !== null) {
      count.dismissed += 1;
    }
  }
  return counts;
}

export function concerns(observation: Observation, id: EntryId): boolean {
  const { focus, subjects } = observation.diagnostic;
  return (
    (focus !== null && sameId(focus, id)) ||
    subjects.some((subject) => sameId(subject, id))
  );
}

// The groups about one entry: each keeps only its selected observations
// whose focus or subjects name it.
export function aboutEntry(
  groups: readonly FindingGroup[],
  id: EntryId,
): FindingGroup[] {
  const about: FindingGroup[] = [];
  for (const group of groups) {
    const selected = group.selected.filter((o) => concerns(o, id));
    if (selected.length > 0) {
      about.push({ ...group, selected });
    }
  }
  return about;
}

// The prefix boundaries an observation was emitted at, read against the
// boundary of its report: "since 3" when the range runs up to that boundary,
// "3–7" or "at 5" when it ended before, and "initialization". A range is
// contiguous, so "since" leaves nothing out; `ended` says only that it was
// not emitted at the boundary, never that anything was resolved (SPEC §8.3).
export function rangeWords(
  evaluated: EvaluatedView,
  boundary: DecimalString,
): { text: string; ended: boolean } {
  if (evaluated.kind === "initialization") {
    return { text: "initialization", ended: false };
  }
  const first = evaluated.first.end_exclusive;
  const last = evaluated.last.end_exclusive;
  if (last === boundary) {
    return { text: `since ${first}`, ended: false };
  }
  return {
    text: first === last ? `at ${first}` : `${first}–${last}`,
    ended: true,
  };
}

// Why a suggestion's actions cannot go into the draft, which applies to the
// current history: they must come from a run over the whole history at the
// current reload, emitted at that run's boundary; null when they can.
export function addBlocker(
  observation: Observation,
  output: AnalysisOutput,
  run: { whole: boolean; current: boolean },
): string | null {
  if (!run.whole) {
    return `this run is of [0, ${output.view.end_exclusive}) only: run over the whole history to add its actions`;
  }
  if (!run.current) {
    return "the screens were reloaded since this run: Run again to add its actions";
  }
  const { evaluated_view } = observation;
  if (
    evaluated_view.kind === "initialization" ||
    evaluated_view.last.end_exclusive !== output.view.end_exclusive
  ) {
    return "not emitted at this run's boundary: its actions were judged against an earlier history";
  }
  return null;
}

// The observation a group is shown by where it takes one line: its newest
// selected one.
export function lead(group: FindingGroup): Observation {
  const observation = group.selected[group.selected.length - 1];
  if (observation === undefined) {
    throw new Error("a shown group has a selected observation");
  }
  return observation;
}

// Groups that read alike, for display only: one analyzer identity, kind,
// and message. Dismissal stays per complete key.
export interface FindingCluster {
  id: string;
  kind: MessageType;
  analyzer: AnalyzerIdentity;
  message: string;
  groups: FindingGroup[];
}

// Clusters in the order of their first group, groups in their own order.
export function clusterGroups(
  groups: readonly FindingGroup[],
): FindingCluster[] {
  const clusters = new Map<string, FindingCluster>();
  for (const group of groups) {
    const { message } = lead(group).diagnostic;
    const { analyzer, kind } = group.key;
    const id = JSON.stringify([
      analyzer.name,
      analyzer.version,
      analyzer.configuration,
      kind,
      message,
    ]);
    const cluster = clusters.get(id);
    if (cluster === undefined) {
      clusters.set(id, { id, kind, analyzer, message, groups: [group] });
    } else {
      cluster.groups.push(group);
    }
  }
  return [...clusters.values()];
}

// Advisories whose shown message or focus label contains `text`, ignoring
// case. Like every advisory filter it never hides an inconsistency.
export function filterText(
  groups: readonly FindingGroup[],
  text: string,
  label: (id: EntryId) => string,
): FindingGroup[] {
  const needle = text.trim().toLowerCase();
  if (needle === "") {
    return [...groups];
  }
  const has = (words: string) => words.toLowerCase().includes(needle);
  return groups.filter(
    (group) =>
      group.key.kind === "inconsistency" ||
      group.selected.some(
        ({ diagnostic }) =>
          has(diagnostic.message) ||
          (diagnostic.focus !== null && has(label(diagnostic.focus))),
      ),
  );
}

// Subjects worth listing beside the focus: none when they only repeat it.
export function shownSubjects<T>(
  focus: T | null,
  subjects: readonly T[],
  same: (a: T, b: T) => boolean,
): readonly T[] {
  if (focus !== null && subjects.every((subject) => same(subject, focus))) {
    return [];
  }
  return subjects;
}

// "12 warnings" or "1 inconsistency, 2 suggestions": only the kinds present.
export function kindWords(counts: { [K in MessageType]: number }): string {
  const words: string[] = [];
  for (const kind of ["inconsistency", "warning", "suggestion"] as const) {
    if (counts[kind] > 0) {
      words.push(counted(counts[kind], kind));
    }
  }
  return words.length === 0 ? "none" : words.join(", ");
}
