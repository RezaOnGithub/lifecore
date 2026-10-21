import type {
  DecimalString,
  EntryId,
  EntryKind,
  ProposalEntry,
} from "../contract.ts";
import { isRecord } from "./guards.ts";
import { entryKey, pastBoundary, sameId } from "./labels.ts";
import { readEntryId } from "./query.ts";
import { compareCodePoints, nameError } from "./values.ts";

// A name kept in this browser for an entry: an applied operation's name, or
// one given in the inspector. A name names one entry; an entry may have
// several. Names follow the operation name grammar, so Revise can name its
// operation after one. Lists are kept in name order.
export interface Alias {
  name: string;
  id: EntryId;
  kind: EntryKind;
  ty_name: string;
}

// The entry a name points at.
export type Target = Omit<Alias, "name">;

// What one applied operation means for the names: `follows` is the entry
// whose names pass to the operation's entry, and a `derived` operation's own
// name names nothing.
export interface AppliedName extends Alias {
  follows: EntryId | null;
  derived: boolean;
}

function byName(a: Alias, b: Alias): number {
  return compareCodePoints(a.name, b.name);
}

function sameAlias(a: Alias, b: Alias): boolean {
  return (
    a.name === b.name &&
    sameId(a.id, b.id) &&
    a.kind === b.kind &&
    a.ty_name === b.ty_name
  );
}

// The names of `id`, in name order.
export function namesFor(aliases: readonly Alias[], id: EntryId): string[] {
  return aliases.filter((alias) => sameId(alias.id, id)).map((a) => a.name);
}

// The names shown beside positions, by entry key. A name of an entry past
// the as-of boundary is not shown there: the entry is not in the view.
export function aliasIndex(
  aliases: readonly Alias[],
  asOf: DecimalString | null,
): Map<string, string[]> {
  const index = new Map<string, string[]>();
  for (const alias of aliases) {
    if (pastBoundary(alias.id, asOf)) {
      continue;
    }
    const key = entryKey(alias.id);
    const names = index.get(key);
    if (names === undefined) {
      index.set(key, [alias.name]);
    } else {
      names.push(alias.name);
    }
  }
  return index;
}

// Why `name` cannot be given to the entry `id`: its grammar, or any entry
// holding it. One `id` already holds is refused too, so that a rename onto
// it does not silently forget the name renamed.
export function aliasNameError(
  aliases: readonly Alias[],
  name: string,
  id: EntryId,
): string | null {
  const syntax = nameError(name);
  if (syntax !== null) {
    return syntax;
  }
  const holder = aliases.find((alias) => alias.name === name);
  if (holder === undefined) {
    return null;
  }
  return sameId(holder.id, id)
    ? `${name} already names this entry`
    : `${name} already names @${holder.id.position}`;
}

// The name of another entry that applying `entry` would take: the
// operation's own name, unless the operation revises that entry, whose names
// pass to it anyway.
export function takenName(
  aliases: readonly Alias[],
  entry: ProposalEntry,
): Alias | null {
  const holder = aliases.find((alias) => alias.name === entry.name);
  if (holder === undefined) {
    return null;
  }
  const revises =
    entry.kind === "node" &&
    entry.intent === "revise" &&
    entry.predecessor !== undefined &&
    "existing" in entry.predecessor &&
    sameId(entry.predecessor.existing, holder.id);
  return revises ? null : holder;
}

// `alias.name` names `alias.id` from now on, wherever it pointed before.
// An unchanged list is returned as is, so nothing is stored again.
export function withAlias(aliases: readonly Alias[], alias: Alias): Alias[] {
  const held = aliases.find((a) => a.name === alias.name);
  if (held !== undefined && sameAlias(held, alias)) {
    return aliases as Alias[];
  }
  return [...aliases.filter((a) => a.name !== alias.name), alias].sort(byName);
}

export function withoutAlias(aliases: readonly Alias[], name: string): Alias[] {
  return aliases.some((alias) => alias.name === name)
    ? aliases.filter((alias) => alias.name !== name)
    : (aliases as Alias[]);
}

// Refused, leaving the list as it was, when `to` breaks the grammar or
// is held, by this entry or another.
export function renameAlias(
  aliases: readonly Alias[],
  from: string,
  to: string,
): Alias[] {
  const alias = aliases.find((a) => a.name === from);
  if (
    alias === undefined ||
    from === to ||
    aliasNameError(aliases, to, alias.id) !== null
  ) {
    return aliases as Alias[];
  }
  return withAlias(withoutAlias(aliases, from), { ...alias, name: to });
}

// Every name of `from` names `to` instead, as when a superseded node's
// names pass to a current descendant.
export function moveAliases(
  aliases: readonly Alias[],
  from: EntryId,
  to: Target,
): Alias[] {
  if (sameId(from, to.id) || !aliases.some((a) => sameId(a.id, from))) {
    return aliases as Alias[];
  }
  return aliases.map((alias) =>
    sameId(alias.id, from) ? { ...to, name: alias.name } : alias,
  );
}

// An apply's names in operation order: an operation first takes over the
// names of the entry it follows, then, unless derived, its own name names
// its entry wherever that name pointed before. A later operation follows
// an earlier one of the same apply, so a chain of revisions passes the
// names along to its end.
export function recordApplied(
  aliases: readonly Alias[],
  applied: readonly AppliedName[],
): Alias[] {
  let next = aliases as Alias[];
  for (const { follows, derived, ...alias } of applied) {
    if (follows !== null) {
      next = moveAliases(next, follows, alias);
    }
    if (!derived) {
      next = withAlias(next, alias);
    }
  }
  return next;
}

function readAlias(value: unknown): Alias | null {
  if (!isRecord(value) || typeof value.name !== "string") {
    return null;
  }
  const id = readEntryId(value.id);
  if (
    id === null ||
    nameError(value.name) !== null ||
    (value.kind !== "node" && value.kind !== "edge") ||
    typeof value.ty_name !== "string"
  ) {
    return null;
  }
  return { name: value.name, id, kind: value.kind, ty_name: value.ty_name };
}

// Whatever of a stored list still reads, in name order; a malformed item is
// dropped, and of a repeated name the first is kept.
export function readAliases(value: unknown): Alias[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const seen = new Set<string>();
  const aliases: Alias[] = [];
  for (const item of value) {
    const alias = readAlias(item);
    if (alias !== null && !seen.has(alias.name)) {
      seen.add(alias.name);
      aliases.push(alias);
    }
  }
  return aliases.sort(byName);
}
