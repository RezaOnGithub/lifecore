import type {
  AnalysisOutput,
  Dismissal,
  EntryId,
  EvaluatedView,
  FindingKey,
  MessageType,
  Observation,
} from "../contract.ts";
import { sameId } from "./labels.ts";

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
  output: AnalysisOutput,
): boolean {
  return observation.diagnostic.sticky && !atBoundary(observation, output);
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

// "boundary 7", "boundaries 3–7", or "initialization": the first and last
// prefix boundaries the observation was emitted at.
export function rangeText(evaluated: EvaluatedView): string {
  if (evaluated.kind === "initialization") {
    return "initialization";
  }
  const first = evaluated.first.end_exclusive;
  const last = evaluated.last.end_exclusive;
  return first === last ? `boundary ${first}` : `boundaries ${first}–${last}`;
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

const KIND_WORDS: { [K in MessageType]: [string, string] } = {
  inconsistency: ["inconsistency", "inconsistencies"],
  warning: ["warning", "warnings"],
  suggestion: ["suggestion", "suggestions"],
};

// "12 warnings" or "1 inconsistency, 2 suggestions": only the kinds present.
export function kindWords(counts: { [K in MessageType]: number }): string {
  const words: string[] = [];
  for (const kind of ["inconsistency", "warning", "suggestion"] as const) {
    const n = counts[kind];
    if (n > 0) {
      const [one, many] = KIND_WORDS[kind];
      words.push(`${n} ${n === 1 ? one : many}`);
    }
  }
  return words.length === 0 ? "none" : words.join(", ");
}
