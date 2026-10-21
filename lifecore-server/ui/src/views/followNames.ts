import type { EntryId, NodeRecord } from "../contract.ts";
import type { Alias, Target } from "../model/aliases.ts";
import { entryKey } from "../model/labels.ts";
import { currentDescendants } from "../model/lineage.ts";
import type { Call } from "../rpc.ts";
import { readLabels } from "./entryLabels.ts";

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
// show. Every read parses the whole snapshot on the server, so the records
// come as labels do, many in one query, and only superseded nodes are
// inspected, one at a time. A lineage is the whole revision tree, so a tree
// is inspected once for all its named nodes. A node that could not be read,
// or that a restore took out of the database, leaves the others to be
// followed. Edges have no successors.
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
  const named = [...nodes.values()];
  // A node left out, by a failed read or a refused seed, is unread.
  const { nodes: records } = await readLabels(
    call,
    named.map(({ id }) => id),
  );
  // The lineage read for each node in it, by key.
  const trees = new Map<string, readonly NodeRecord[]>();
  // In the order of the names.
  const followed: Followed = { moves: [], split: [], unread: [] };
  for (const { id, names } of named) {
    const key = entryKey(id);
    const record = records.get(key);
    if (record === undefined) {
      followed.unread.push(...names);
      continue;
    }
    if (record.current) {
      continue;
    }
    let lineage = trees.get(key);
    if (lineage === undefined) {
      const inspection = await call("entries.inspect", { id }).catch(
        () => null,
      );
      if (inspection?.kind !== "node") {
        followed.unread.push(...names);
        continue;
      }
      lineage = inspection.lineage;
      for (const node of lineage) {
        trees.set(entryKey(node.id), lineage);
      }
    }
    const current: Target[] = currentDescendants(lineage, id).map((node) => ({
      id: node.id,
      kind: "node",
      ty_name: node.ty_name,
    }));
    const [only] = current;
    if (only !== undefined && current.length === 1) {
      followed.moves.push({ from: id, to: only, names });
    } else {
      followed.split.push({ id, names, count: current.length });
    }
  }
  return followed;
}
