import type {
  AnalysisRun,
  DecimalString,
  EntryId,
  NodeRecord,
  ViewQuery,
} from "../contract.ts";
import { entryKey } from "../model/labels.ts";
import { type Call, RpcError } from "../rpc.ts";

// Below this many, labels are read one entry at a time.
const FEW = 20;

// The keys of the ids a `views.query` refused as seeds, if it did.
function refusedKeys(failure: unknown): Set<string> | null {
  if (failure instanceof RpcError && "data" in failure.error) {
    const { data } = failure.error;
    return data.kind === "invalid_seeds"
      ? new Set(data.refused.map(({ id }) => entryKey(id)))
      : null;
  }
  return null;
}

// The entries a run names: focus, subjects, and suggested references.
export function namedIds(run: AnalysisRun): EntryId[] {
  const ids: EntryId[] = [];
  for (const { diagnostic } of run.analysis.observations) {
    if (diagnostic.focus !== null) {
      ids.push(diagnostic.focus);
    }
    ids.push(...diagnostic.subjects);
    if (diagnostic.kind === "suggestion") {
      for (const action of diagnostic.actions) {
        const references =
          action.kind === "edge"
            ? [action.from, action.to]
            : action.predecessor === undefined
              ? []
              : [action.predecessor];
        for (const reference of references) {
          if ("existing" in reference) {
            ids.push(reference.existing);
          }
        }
      }
    }
  }
  return ids;
}

// The nodes among `ids` in the view [0, as_of), or the whole history
// without `as_of`, for labels; edges and anything else stay positions. A few
// are read one by one; more with one `views.query`, which seeds only nodes,
// though nothing tells an edge apart beforehand. Its refusal names every id
// that is not a node, so one retry without them reads the rest.
export async function readLabels(
  call: Call,
  ids: readonly EntryId[],
  as_of?: DecimalString,
): Promise<{ nodes: Map<string, NodeRecord>; failure: unknown }> {
  const wanted = new Map<string, EntryId>();
  for (const id of ids) {
    wanted.set(entryKey(id), id);
  }
  const bound = as_of === undefined ? {} : { as_of };
  const nodes = new Map<string, NodeRecord>();
  if (wanted.size < FEW) {
    let failure: unknown = null;
    const answers = await Promise.allSettled(
      [...wanted.values()].map((id) => call("entries.get", { id, ...bound })),
    );
    for (const answer of answers) {
      if (answer.status === "rejected") {
        failure ??= answer.reason;
      } else if (answer.value.entry.kind === "node") {
        nodes.set(entryKey(answer.value.entry.id), answer.value.entry);
      }
    }
    return { nodes, failure };
  }
  const ask = (seeds: EntryId[]) =>
    call("views.query", { seeds: [{ kind: "ids", ids: seeds }], ...bound });
  let answer: ViewQuery;
  try {
    answer = await ask([...wanted.values()]);
  } catch (failure) {
    const refused = refusedKeys(failure);
    if (refused === null) {
      return { nodes, failure };
    }
    const rest = [...wanted]
      .filter(([key]) => !refused.has(key))
      .map(([, id]) => id);
    if (rest.length === 0) {
      return { nodes, failure: null };
    }
    try {
      answer = await ask(rest);
    } catch (failure) {
      return { nodes, failure };
    }
  }
  for (const { node } of answer.nodes) {
    nodes.set(entryKey(node.id), node);
  }
  return { nodes, failure: null };
}
