import { type KeyboardEvent, useState } from "react";
import type { EntryRecord, NodeRecord } from "../contract.ts";
import { useData, usePlaces } from "../data.tsx";
import { useOpen } from "../links.tsx";
import { aliasNameError, namesFor } from "../model/aliases.ts";
import {
  labelField,
  pastBoundary,
  rowLabel,
  sameId,
  typeName,
} from "../model/labels.ts";
import { currentDescendants } from "../model/lineage.ts";
import { truncate } from "../model/payload.ts";
import type { Screen } from "../navigation.ts";
import { type Followed, followNames } from "./followNames.ts";

// Names are cut like sidebar places; a descendant's label in a move button
// like a relationship cell.
const NAME_LIMIT = 40;
const TARGET_LIMIT = 30;

function listed(names: readonly string[]): string {
  return names.length === 1
    ? (names[0] ?? "")
    : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

// The sidebar's names, in name order, each opening its entry. A name of an
// entry past the as-of boundary is the user's own, so it stays, dimmed and
// without its type. Follow revisions reads the current state and moves the
// names of superseded nodes that have one current descendant.
export function NameList({ screen }: { screen: Screen }) {
  const { places, removeAlias, moveAliases } = usePlaces();
  const { asOf, client } = useData();
  const open = useOpen();
  const [following, setFollowing] = useState(false);
  const [followed, setFollowed] = useState<Followed | null>(null);
  const { aliases } = places;
  if (aliases.length === 0) {
    return (
      <p className="muted side-hint">
        applied operations and named entries appear here
      </p>
    );
  }
  const follow = async () => {
    setFollowing(true);
    const result = await followNames(
      (method, params) => client.call(method, params),
      aliases,
    );
    for (const move of result.moves) {
      moveAliases(move.from, move.to);
    }
    setFollowed(result);
    setFollowing(false);
  };
  return (
    <>
      <ul className="types places" data-testid="names">
        {aliases.map((alias) => {
          const past = pastBoundary(alias.id, asOf);
          const position = `@${alias.id.position}`;
          const active =
            screen.kind === "inspector" && sameId(screen.id, alias.id);
          return (
            <li
              key={alias.name}
              className={
                [active && "active", past && "beyond"]
                  .filter(Boolean)
                  .join(" ") || undefined
              }
            >
              <button
                type="button"
                className="link place-label"
                title={
                  past
                    ? `${alias.name}\n${position} past the as-of boundary`
                    : `${alias.name}\n${position} ${
                        alias.kind === "edge" ? "edge " : ""
                      }${typeName(alias.ty_name)}`
                }
                onClick={() => open({ kind: "inspector", id: alias.id })}
              >
                {truncate(alias.name, NAME_LIMIT)}
              </button>
              <span className="place-type">{position}</span>
              <button
                type="button"
                className="link remove"
                aria-label={`forget the name ${alias.name}`}
                onClick={() => removeAlias(alias.name)}
              >
                ×
              </button>
            </li>
          );
        })}
      </ul>
      <p className="side-hint">
        <button
          type="button"
          className="link small"
          disabled={following}
          title="read the current state, and move the names of each superseded node to its current revision"
          onClick={() => void follow()}
        >
          {following ? "Following…" : "Follow revisions"}
        </button>
      </p>
      {followed !== null && !following && (
        <FollowReport followed={followed} onDismiss={() => setFollowed(null)} />
      )}
    </>
  );
}

function FollowReport({
  followed,
  onDismiss,
}: {
  followed: Followed;
  onDismiss: () => void;
}) {
  const open = useOpen();
  const { moves, split, unread } = followed;
  const moved = moves.flatMap((move) => move.names);
  return (
    <div className="side-hint" data-testid="follow-report">
      {moved.length === 0 && split.length === 0 && unread.length === 0 && (
        <p>Every name is on a current entry.</p>
      )}
      {moved.length > 0 && <p>Moved {listed(moved)}.</p>}
      {split.map((node) => (
        <p key={node.names[0]}>
          <button
            type="button"
            className="link"
            onClick={() => open({ kind: "inspector", id: node.id })}
          >
            {listed(node.names)}
          </button>
          : {node.count} current revisions, to choose from.
        </p>
      ))}
      {unread.length > 0 && <p>Could not read {listed(unread)}.</p>}
      <button type="button" className="link small" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}

// The inline field that names the entry or renames one of its names.
function NameField({
  initial,
  error,
  onSave,
  onCancel,
}: {
  initial: string;
  error: (text: string) => string | null;
  onSave: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(initial);
  const problem = text === "" ? null : error(text);
  const save = () => {
    if (text !== "" && problem === null) {
      onSave(text);
    }
  };
  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save();
    } else if (event.key === "Escape") {
      onCancel();
    }
  };
  return (
    <span className="save-view">
      <input
        aria-label="entry name"
        value={text}
        size={20}
        aria-invalid={problem !== null}
        // biome-ignore lint/a11y/noAutofocus: the field opens on request
        autoFocus
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKey}
      />
      <button
        type="button"
        disabled={text === "" || problem !== null}
        onClick={save}
      >
        Save
      </button>
      <button type="button" onClick={onCancel}>
        Cancel
      </button>
      {problem !== null && <span className="field-error">{problem}</span>}
    </span>
  );
}

// The inspected entry's names, to rename, forget, or add to. For a node a
// read of the whole history shows superseded, its names can move to a
// current descendant; under an as-of boundary the read does not say what is
// current in the whole history, so the move waits until the boundary is
// cleared. `settled` is false while an earlier read stands in for this one.
export function EntryNames({
  entry,
  lineage = [],
  settled,
}: {
  entry: EntryRecord;
  lineage?: readonly NodeRecord[];
  settled: boolean;
}) {
  const { places, nameEntry, renameAlias, removeAlias, moveAliases } =
    usePlaces();
  const { asOf } = useData();
  const [editing, setEditing] = useState<{ from: string | null } | null>(null);
  const names = namesFor(places.aliases, entry.id);
  const error = (from: string | null) => (text: string) =>
    text === from ? null : aliasNameError(places.aliases, text, entry.id);
  const save = (from: string | null) => (text: string) => {
    if (from === null) {
      nameEntry({
        name: text,
        id: entry.id,
        kind: entry.kind,
        ty_name: entry.ty_name,
      });
    } else {
      renameAlias(from, text);
    }
    setEditing(null);
  };
  const descendants =
    entry.kind === "node" && settled && !entry.current && names.length > 0
      ? currentDescendants(lineage, entry.id)
      : [];
  const bounded = asOf !== null;
  return (
    <>
      <div className="row names" data-testid="entry-names">
        <span className="muted">names</span>
        {names.map((name) =>
          editing?.from === name ? (
            <NameField
              key={name}
              initial={name}
              error={error(name)}
              onSave={save(name)}
              onCancel={() => setEditing(null)}
            />
          ) : (
            <span key={name} className="name-chip" data-testid="entry-name">
              <span className="alias">{name}</span>{" "}
              <button
                type="button"
                className="link small"
                aria-label={`rename ${name}`}
                title="rename"
                onClick={() => setEditing({ from: name })}
              >
                ✎
              </button>{" "}
              <button
                type="button"
                className="link small remove"
                aria-label={`forget the name ${name}`}
                title="forget this name"
                onClick={() => removeAlias(name)}
              >
                ×
              </button>
            </span>
          ),
        )}
        {editing?.from === null ? (
          <NameField
            initial=""
            error={error(null)}
            onSave={save(null)}
            onCancel={() => setEditing(null)}
          />
        ) : (
          <button
            type="button"
            className="link small"
            title="keep a name for this entry in this browser"
            onClick={() => setEditing({ from: null })}
          >
            {names.length === 0 ? "name this entry" : "add a name"}
          </button>
        )}
      </div>
      {descendants.length > 0 && (
        <div className="banner names-follow" data-testid="names-follow">
          This node is superseded: {listed(names)} can move to{" "}
          {descendants.length === 1
            ? "its current revision"
            : `one of its ${descendants.length} current revisions`}
          .{" "}
          {descendants.map((node) => (
            <button
              key={node.id.position}
              type="button"
              disabled={bounded}
              title={
                bounded
                  ? "a bounded read does not say what is current in the whole history; clear the as-of boundary (current) to move the names"
                  : `the names name @${node.id.position} from now on`
              }
              onClick={() =>
                moveAliases(entry.id, {
                  id: node.id,
                  kind: "node",
                  ty_name: node.ty_name,
                })
              }
            >
              Move to {truncate(rowLabel(node), TARGET_LIMIT)}
              {labelField(node) !== null && ` @${node.id.position}`}
            </button>
          ))}
        </div>
      )}
    </>
  );
}
