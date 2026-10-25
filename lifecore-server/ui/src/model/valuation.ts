import type {
  AnalyzerConfig,
  AsOfParams,
  EntryId,
  Ranked,
  Reason,
  ValuationParams,
} from "../contract.ts";

export type Tier = Ranked["tier"];

// The tiers in order of pressure, named by what their members have in
// common (PLANNING §9); the summary says how a tier's rows are ordered,
// which the server decides and the client only shows.
export const TIERS: readonly {
  tier: Tier;
  title: string;
  summary: string;
}[] = [
  {
    tier: 0,
    title: "What the next seal would reject",
    summary:
      "Undecided items of the frontier, and goals and projects whose plan is broken: goals and concerns first, then by how many goals in play each reaches.",
  },
  {
    tier: 1,
    title: "Standing commitments",
    summary:
      "Open tasks committed for this window, longest open first, then by nearest deadline. Each commitment lapses at the next seal, which asks for a new decision.",
  },
  {
    tier: 2,
    title: "Habits and concerns below quota",
    summary:
      "Short of their quota so far this window, the largest shortfall first.",
  },
  {
    tier: 3,
    title: "Deferred or waiting",
    summary:
      "Set aside for now, those lapsing at the next seal first, then longest open. What is dormant only through them is not ranked.",
  },
  {
    tier: 4,
    title: "Starved goals and projects",
    summary:
      "In play, with no committed task in their strict subgraph, the longest starved first.",
  },
  {
    tier: 5,
    title: "Everything else in play",
    summary:
      "Longest in the plan first; a habit or concern in its grace period is new.",
  },
];

export interface TierGroup {
  tier: Tier;
  title: string;
  summary: string;
  rows: Ranked[];
}

// The rows under their tiers, each tier keeping the server's order, which
// is the ranking; the client never re-ranks. Tiers without rows are listed
// apart, so the screen can name them without a heading each.
export function tierGroups(ranked: readonly Ranked[]): {
  shown: TierGroup[];
  empty: Tier[];
} {
  const shown: TierGroup[] = [];
  const empty: Tier[] = [];
  for (const { tier, title, summary } of TIERS) {
    const rows = ranked.filter((row) => row.tier === tier);
    if (rows.length === 0) {
      empty.push(tier);
    } else {
      shown.push({ tier, title, summary, rows });
    }
  }
  return { shown, empty };
}

function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

// "tier 1 (standing commitments) or tier 4 (starved goals and projects)".
export function tierList(tiers: readonly Tier[]): string {
  const words = tiers.map((tier) => {
    const title = TIERS[tier]?.title ?? "";
    return `tier ${tier} (${lowerFirst(title)})`;
  });
  if (words.length <= 2) {
    return words.join(" or ");
  }
  return `${words.slice(0, -1).join(", ")}, or ${words.at(-1)}`;
}

// How a reason stands out, read from its structured value; its words are
// always the server's text. Urgent: what the next seal would reject, and a
// deadline already passed. Soon: a deferral or wait that the next seal ends.
export type Emphasis = "urgent" | "soon" | null;

export function emphasis(reason: Reason): Emphasis {
  switch (reason.name) {
    case "undecided":
    case "broken_plan":
    case "deadline_passed":
      return "urgent";
    case "lapses_at_next_seal":
      return "soon";
    default:
      return null;
  }
}

export function rankedIds(ranked: readonly Ranked[]): EntryId[] {
  return ranked.map((row) => row.revision);
}

// The deadline sample as typed: empty for the server's default, else a
// whole number of windows, which travels as a JSON number (CLIENT §3), so
// one past what a JavaScript number holds exactly is refused here.
export type SampleChoice =
  | { kind: "default" }
  | { kind: "windows"; windows: number }
  | { kind: "invalid"; problem: string };

const DIGITS = /^[0-9]+$/;

export function sampleChoice(text: string): SampleChoice {
  const trimmed = text.trim();
  if (trimmed === "") {
    return { kind: "default" };
  }
  if (!DIGITS.test(trimmed)) {
    return { kind: "invalid", problem: "a whole number of windows, 0 or more" };
  }
  const windows = Number(trimmed);
  return Number.isSafeInteger(windows)
    ? { kind: "windows", windows }
    : { kind: "invalid", problem: "too many windows" };
}

// The request: the top bar's boundary, and the sample only when one is
// chosen, so the default is the server's.
export function valuationParams(
  asOf: AsOfParams,
  windows: number | null,
): ValuationParams {
  return windows === null
    ? { ...asOf }
    : { ...asOf, deadline_sample_windows: windows };
}

// The registered name of the planning analyzer. The ranking never reads
// the suite; whether it holds Planning only says whether seals are judged.
export const PLANNING = "Planning";

export function configuresPlanning(suite: readonly AnalyzerConfig[]): boolean {
  return suite.some((analyzer) => analyzer.name === PLANNING);
}
