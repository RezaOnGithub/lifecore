import type { EntryId } from "../contract.ts";
import type { Alias, Target } from "../model/aliases.ts";
import { entryKey } from "../model/labels.ts";
import { currentDescendants } from "../model/lineage.ts";
import type { Call } from "../rpc.ts";

// What a read of the current state says about the named nodes that it
// supersedes: those with one current descendant, whose names move there,
// and those with several, among which the user chooses in the inspector.
// `unread` are the names of nodes that could not be read.
export interface Followed {
  moves: { from: EntryId; to: Target; names: string[] }[];
  split: { id: EntryId; names: string[]; count: number }[];
  unread: string[];
}

// Reads the named nodes without `as_of`: a superseded node's names move to
// a node current in the state drafts apply to, whatever boundary is on
// show. Each node is read on its own, so one that fails, or that a restore
// took out of the database, leaves the others to be followed; only a
// superseded one is inspected for its lineage. Edges have no successors.
export async function followNames(
  call: Call,
  aliases: readonly Alias[],
): Promise<Followed> {
  const nodes = new Map<string, { id: EntryId; names: string[] }>();
  for (const alias of aliases) {
    if (alias.kind !== "node") {
      continue;
    }
    const key = entryKey(alias.id);
    const known = nodes.get(key);
    if (known === undefined) {
      nodes.set(key, { id: alias.id, names: [alias.name] });
    } else {
      known.names.push(alias.name);
    }
  }
  type Answer =
    | { kind: "current" }
    | { kind: "descendants"; current: Target[] };
  const named = [...nodes.values()];
  const answers = await Promise.allSettled(
    named.map(async ({ id }): Promise<Answer> => {
      const { entry } = await call("entries.get", { id });
      if (entry.kind !== "node" || entry.current) {
        return { kind: "current" };
      }
      const inspection = await call("entries.inspect", { id });
      if (inspection.kind !== "node") {
        return { kind: "current" };
      }
      return {
        kind: "descendants",
        current: currentDescendants(inspection.lineage, id).map((node) => ({
          id: node.id,
          kind: "node",
          ty_name: node.ty_name,
        })),
      };
    }),
  );
  // In the order of the names, whichever answer came first.
  const followed: Followed = { moves: [], split: [], unread: [] };
  named.forEach(({ id, names }, index) => {
    const answer = answers[index];
    if (answer === undefined || answer.status === "rejected") {
      followed.unread.push(...names);
      return;
    }
    if (answer.value.kind === "current") {
      return;
    }
    const { current } = answer.value;
    const [only] = current;
    if (only !== undefined && current.length === 1) {
      followed.moves.push({ from: id, to: only, names });
    } else {
      followed.split.push({ id, names, count: current.length });
    }
  });
  return followed;
}
