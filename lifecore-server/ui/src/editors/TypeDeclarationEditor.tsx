import { useEffect, useId, useRef, useState } from "react";
import type {
  Body,
  FieldDecl,
  FieldDecls,
  FieldStatus,
  Shape,
  TypeSchema,
} from "../contract.ts";
import { shapeWords } from "../model/payload.ts";
import {
  DECLARATION_KEYS,
  type DeclarationReading,
  declarationBody,
  declarationForForm,
  MAX_SHAPE_DEPTH,
  PRESENCE_WORDS,
  schemaErrorMessage,
  schemaPathText,
  shapeDepth,
  shapeWordList,
} from "../model/schema.ts";
import { compareCodePoints, getOwn } from "../model/values.ts";
import {
  BUILDER_KINDS,
  type BuilderKind,
  initialShape,
  isBuilderKind,
  liveDraft,
  memberNameError,
  namedKeys,
  needsFirstName,
  parseTags,
  renameField,
  renameKey,
  renameMember,
  sameMap,
  shapeOfKind,
  type TextDraft,
  withField,
  withoutField,
} from "./helpers.ts";
import { ModeToggle, NoEditor, ValidatedInput } from "./inputs.tsx";
import { RawBodyEditor, useRawBody } from "./RawJsonEditor.tsx";

function firstWord(kind: "enum" | "variant"): "member" | "tag" {
  return kind === "enum" ? "member" : "tag";
}

// The first member or tag of an enum or variant chosen in the builder,
// which is not a shape until it has one (FILES §2.6). The input takes
// focus; Escape or cancel keeps the shape as it was.
function FirstName({
  field,
  kind,
  onSet,
  onCancel,
}: {
  field: string;
  kind: "enum" | "variant";
  onSet: (name: string) => void;
  onCancel: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const word = firstWord(kind);
  useEffect(() => {
    input.current?.focus();
  }, []);
  const submit = () => {
    if (name !== "") {
      onSet(name);
    }
  };
  return (
    <span className="row first-name">
      <input
        ref={input}
        type="text"
        size={14}
        aria-label={`${field} first ${word}`}
        placeholder={`first ${word}`}
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          } else if (event.key === "Escape") {
            onCancel();
          }
        }}
      />
      <button
        type="button"
        aria-label={`make ${kind} of ${field}`}
        disabled={name === ""}
        onClick={submit}
      >
        make {kind}
      </button>
      <button
        type="button"
        aria-label={`cancel ${kind} for ${field}`}
        onClick={onCancel}
      >
        cancel
      </button>
    </span>
  );
}

// An enum's members, one input each, in code point order as the server's
// set holds them. A rename keeps the row (and its focus) while the member
// moves to its place; the last member cannot be removed, since an enum
// needs one.
function MembersEditor({
  field,
  members,
  onChange,
}: {
  field: string;
  members: string[];
  onChange: (members: string[]) => void;
}) {
  const [keys, setKeys] = useState<ReadonlyMap<string, number>>(
    () => new Map(),
  );
  const [name, setName] = useState("");
  const rowKeys = namedKeys(members, keys);
  if (!sameMap(rowKeys, keys)) {
    setKeys(rowKeys);
  }
  const error =
    name === "" ? null : memberNameError(name, members, null, "member");
  const add = () => {
    if (name !== "" && error === null) {
      onChange([...members, name].sort(compareCodePoints));
      setName("");
    }
  };
  return (
    <div className="members-editor">
      {members.map((member, index) => (
        <span className="member" key={rowKeys.get(member)}>
          <ValidatedInput
            label={`${field} member ${index + 1}`}
            value={member}
            size={12}
            validate={(text) =>
              memberNameError(text, members, member, "member")
            }
            onCommit={(text) => {
              setKeys(renameKey(rowKeys, member, text));
              onChange(renameMember(members, member, text));
            }}
          />
          <button
            type="button"
            aria-label={`remove ${member} from ${field}`}
            disabled={members.length === 1}
            title={
              members.length === 1
                ? "an enum needs at least one member"
                : undefined
            }
            onClick={() => onChange(members.filter((m) => m !== member))}
          >
            ×
          </button>
        </span>
      ))}
      <span className="member">
        <input
          type="text"
          size={12}
          aria-label={`new member of ${field}`}
          placeholder="new member"
          value={name}
          aria-invalid={error !== null}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <button
          type="button"
          aria-label={`add member to ${field}`}
          disabled={name === "" || error !== null}
          onClick={add}
        >
          add member
        </button>
        {error !== null && <span className="field-error">{error}</span>}
      </span>
    </div>
  );
}

// A variant's tags in code point order, each with its name and its own
// field declarations, whose paths read `decision(committed).cost` as the
// server's do. The last tag cannot be removed, since a variant needs one.
function TagsEditor({
  field,
  tags,
  onChange,
}: {
  field: string;
  tags: { [tag: string]: FieldDecls };
  onChange: (tags: { [tag: string]: FieldDecls }) => void;
}) {
  const names = Object.keys(tags).sort(compareCodePoints);
  const [keys, setKeys] = useState<ReadonlyMap<string, number>>(
    () => new Map(),
  );
  const [name, setName] = useState("");
  const rowKeys = namedKeys(names, keys);
  if (!sameMap(rowKeys, keys)) {
    setKeys(rowKeys);
  }
  const error = name === "" ? null : memberNameError(name, names, null, "tag");
  const add = () => {
    if (name !== "" && error === null) {
      onChange(withField(tags, name, {}));
      setName("");
    }
  };
  return (
    <div className="tags-editor">
      {names.map((tag, index) => (
        <div className="variant-tag" key={rowKeys.get(tag)}>
          <div className="row">
            <ValidatedInput
              label={`${field} tag ${index + 1}`}
              value={tag}
              size={14}
              validate={(text) => memberNameError(text, names, tag, "tag")}
              onCommit={(text) => {
                setKeys(renameKey(rowKeys, tag, text));
                onChange(renameField(tags, tag, text));
              }}
            />
            <button
              type="button"
              aria-label={`remove tag ${tag} from ${field}`}
              disabled={names.length === 1}
              title={
                names.length === 1
                  ? "a variant needs at least one tag"
                  : undefined
              }
              onClick={() => onChange(withoutField(tags, tag))}
            >
              remove tag
            </button>
          </div>
          <FieldDeclsTable
            fields={getOwn(tags, tag) ?? {}}
            prefix={`${field}(${tag})`}
            onChange={(fields) => onChange(withField(tags, tag, fields))}
          />
        </div>
      ))}
      <div className="row">
        <input
          type="text"
          size={14}
          aria-label={`new tag of ${field}`}
          placeholder="new tag"
          value={name}
          aria-invalid={error !== null}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <button
          type="button"
          aria-label={`add tag to ${field}`}
          disabled={name === "" || error !== null}
          onClick={add}
        >
          add tag
        </button>
        {error !== null && <span className="field-error">{error}</span>}
      </div>
    </div>
  );
}

function ShapeBuilder({
  field,
  shape,
  onChange,
}: {
  field: string;
  shape: Shape;
  onChange: (shape: Shape) => void;
}) {
  // An enum or variant chosen over another shape, waiting for its first
  // member or tag; the declaration keeps the old shape until then.
  const [pending, setPending] = useState<"enum" | "variant" | null>(null);
  // A word the builder does not offer keeps its declaration as it is,
  // shown as its words; the field's name and presence stay editable.
  if (!isBuilderKind(shape.kind)) {
    return (
      <NoEditor
        label={`${field} shape`}
        json={shapeWordList(shape)}
        what={shapeWords(shape)}
      />
    );
  }
  const shown = pending ?? shape.kind;
  return (
    <div className="shape-builder">
      <select
        aria-label={`${field} shape`}
        value={shown}
        onChange={(event) => {
          const kind = event.target.value as BuilderKind;
          const next = shapeOfKind(kind, shape);
          if (next === null) {
            setPending(needsFirstName(kind) ? kind : null);
          } else {
            setPending(null);
            onChange(next);
          }
        }}
      >
        {BUILDER_KINDS.map((kind) => (
          <option key={kind} value={kind}>
            {kind}
          </option>
        ))}
      </select>
      {pending !== null && (
        <FirstName
          field={field}
          kind={pending}
          onSet={(name) => {
            setPending(null);
            onChange(shapeOfKind(pending, shape, name) ?? shape);
          }}
          onCancel={() => setPending(null)}
        />
      )}
      {pending === null && shape.kind === "list" && (
        <>
          <span className="muted"> of </span>
          <ShapeBuilder
            field={`${field} element`}
            shape={shape.element}
            onChange={(element) => onChange({ kind: "list", element })}
          />
        </>
      )}
      {pending === null && shape.kind === "record" && (
        <FieldDeclsTable
          fields={shape.fields}
          prefix={field}
          onChange={(fields) => onChange({ kind: "record", fields })}
        />
      )}
      {pending === null && shape.kind === "enum" && (
        <MembersEditor
          field={field}
          members={shape.members}
          onChange={(members) => onChange({ kind: "enum", members })}
        />
      )}
      {pending === null && shape.kind === "variant" && (
        <TagsEditor
          field={field}
          tags={shape.tags}
          onChange={(tags) => onChange({ kind: "variant", tags })}
        />
      )}
    </div>
  );
}

// A name, a presence, and a shape, added with Enter or the button; the name
// input takes focus again for the next one.
function AddDeclaration({
  fields,
  prefix,
  onAdd,
}: {
  fields: FieldDecls;
  prefix: string | null;
  onAdd: (field: string, decl: FieldDecl) => void;
}) {
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [status, setStatus] = useState<FieldStatus>("optional");
  const [kind, setKind] = useState<BuilderKind>("text");
  // An enum's first member or a variant's first tag, which it needs.
  const [first, setFirst] = useState("");
  const what =
    prefix === null
      ? "new field declaration"
      : `new field declaration in ${prefix}`;
  const error =
    name !== "" && Object.hasOwn(fields, name)
      ? `another field is named ${name}`
      : null;
  const needsFirst = needsFirstName(kind);
  const ready = name !== "" && error === null && (!needsFirst || first !== "");
  const submit = () => {
    if (!ready) {
      return;
    }
    onAdd(name, { status, shape: initialShape(kind, first) });
    setName("");
    setFirst("");
    input.current?.focus();
  };
  return (
    <div className="row add-decl">
      <label htmlFor={`${id}-name`}>{what}</label>
      <input
        ref={input}
        id={`${id}-name`}
        type="text"
        size={16}
        value={name}
        aria-invalid={error !== null}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
      />
      <select
        aria-label={`${what} presence`}
        value={status}
        onChange={(event) => setStatus(event.target.value as FieldStatus)}
      >
        {PRESENCE_WORDS.map((word) => (
          <option key={word} value={word}>
            {word}
          </option>
        ))}
      </select>
      <select
        aria-label={`${what} shape`}
        value={kind}
        onChange={(event) => setKind(event.target.value as BuilderKind)}
      >
        {BUILDER_KINDS.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
      {needsFirst && (
        <input
          type="text"
          size={14}
          aria-label={`${what} first ${firstWord(kind)}`}
          placeholder={`first ${firstWord(kind)}`}
          value={first}
          onChange={(event) => setFirst(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
        />
      )}
      <button type="button" disabled={!ready} onClick={submit}>
        {prefix === null
          ? "Add field declaration"
          : `Add field declaration to ${prefix}`}
      </button>
      {error !== null && <span className="field-error">{error}</span>}
    </div>
  );
}

function FieldDeclsTable({
  fields,
  prefix,
  onChange,
}: {
  fields: FieldDecls;
  prefix: string | null;
  onChange: (fields: FieldDecls) => void;
}) {
  const entries = Object.entries(fields);
  // Rows are keyed by a local id that follows a rename, so the name input
  // keeps focus while its field's name, and maybe its place, changes.
  const [keys, setKeys] = useState<ReadonlyMap<string, number>>(
    () => new Map(),
  );
  const rowKeys = namedKeys(
    entries.map(([field]) => field),
    keys,
  );
  if (!sameMap(rowKeys, keys)) {
    setKeys(rowKeys);
  }
  const labelOf = (field: string) =>
    prefix === null ? field : `${prefix}.${field}`;
  return (
    <div className={prefix === null ? "decls-editor" : "decls-editor nested"}>
      {entries.length === 0 ? (
        <span className="muted">no fields</span>
      ) : (
        <table className="fields">
          <thead>
            <tr>
              <th scope="col">field</th>
              <th scope="col">presence</th>
              <th scope="col">shape</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {entries.map(([field, decl]) => (
              <tr key={rowKeys.get(field)}>
                <td>
                  <ValidatedInput
                    label={`${labelOf(field)} name`}
                    value={field}
                    size={16}
                    validate={(text) =>
                      text === ""
                        ? "name required"
                        : text !== field && Object.hasOwn(fields, text)
                          ? `another field is named ${text}`
                          : null
                    }
                    onCommit={(text) => {
                      setKeys(renameKey(rowKeys, field, text));
                      onChange(renameField(fields, field, text));
                    }}
                  />
                </td>
                <td>
                  <select
                    aria-label={`${labelOf(field)} presence`}
                    value={decl.status}
                    onChange={(event) =>
                      onChange(
                        withField(fields, field, {
                          ...decl,
                          status: event.target.value as FieldStatus,
                        }),
                      )
                    }
                  >
                    {PRESENCE_WORDS.map((word) => (
                      <option key={word} value={word}>
                        {word}
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <ShapeBuilder
                    field={labelOf(field)}
                    shape={decl.shape}
                    onChange={(shape) =>
                      onChange(withField(fields, field, { ...decl, shape }))
                    }
                  />
                  {prefix === null &&
                    shapeDepth(decl.shape) > MAX_SHAPE_DEPTH && (
                      <span className="field-error">
                        more than {MAX_SHAPE_DEPTH} nested list, record, and
                        variant words; the server rejects the declaration
                      </span>
                    )}
                </td>
                <td>
                  <button
                    type="button"
                    aria-label={`remove ${labelOf(field)}`}
                    onClick={() => onChange(withoutField(fields, field))}
                  >
                    remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <AddDeclaration
        fields={fields}
        prefix={prefix}
        onAdd={(field, decl) => onChange(withField(fields, field, decl))}
      />
    </div>
  );
}

// Comma-separated names. The text is local while typing, since the
// committed list comes back sorted and without the comma being typed; when
// focus leaves, the input shows the committed list.
function TagInput({
  label,
  values,
  onChange,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState<TextDraft | null>(null);
  const [note, setNote] = useState<TextDraft | null>(null);
  const committed = values.join(",");
  const live = liveDraft(draft, committed);
  const text = live?.text ?? values.join(", ");
  const repeated = live === null ? [] : parseTags(live.text).repeated;
  const shownNote =
    repeated.length > 0
      ? `${repeated.join(", ")} repeated; written once`
      : (liveDraft(note, committed)?.text ?? null);
  return (
    <>
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="text"
        size={28}
        value={text}
        placeholder="comma-separated"
        onChange={(event) => {
          const next = event.target.value;
          const parsed = parseTags(next);
          setDraft({ text: next, base: parsed.tags.join(",") });
          setNote(null);
          onChange(parsed.tags);
        }}
        onBlur={() => {
          if (live !== null) {
            setDraft(null);
            if (repeated.length > 0) {
              setNote({
                text: `removed repeated ${repeated.join(", ")}`,
                base: committed,
              });
            }
          }
        }}
      />
      {shownNote !== null && <span className="edit-note">{shownNote}</span>}
    </>
  );
}

function nameProblem(name: string): string | null {
  if (name === "") {
    return "a declaration needs a name";
  }
  return name === "Type"
    ? "Type is the built-in metatype; a declaration may not claim it"
    : null;
}

function DeclarationForm({
  schema,
  named,
  onChange,
}: {
  schema: TypeSchema;
  // False for a payload without `name` yet, such as a new `Type` node's.
  named: boolean;
  onChange: (schema: TypeSchema) => void;
}) {
  const id = useId();
  const problem = nameProblem(schema.name);
  return (
    <>
      {!named && (
        <p className="muted">
          A new declaration: its payload is empty until the first edit, which
          writes all five keys.
        </p>
      )}
      <div className="row">
        <label htmlFor={`${id}-name`}>declared name</label>
        <input
          id={`${id}-name`}
          type="text"
          size={24}
          value={schema.name}
          aria-invalid={problem !== null}
          onChange={(event) =>
            onChange({ ...schema, name: event.target.value })
          }
        />
        {problem !== null && <span className="field-error">{problem}</span>}
      </div>
      <FieldDeclsTable
        fields={schema.fields}
        prefix={null}
        onChange={(fields) => onChange({ ...schema, fields })}
      />
      <div className="row">
        <TagInput
          label="outgoing"
          values={schema.outgoing}
          onChange={(outgoing) => onChange({ ...schema, outgoing })}
        />
        <TagInput
          label="incoming"
          values={schema.incoming}
          onChange={(incoming) => onChange({ ...schema, incoming })}
        />
        <TagInput
          label="successors"
          values={schema.successors}
          onChange={(successors) => onChange({ ...schema, successors })}
        />
      </div>
    </>
  );
}

function rawNote(reading: DeclarationReading): string | undefined {
  switch (reading.kind) {
    case "ok":
      return undefined;
    case "unknown_keys":
      return `The payload has ${reading.keys.length === 1 ? "a key" : "keys"} outside the declaration: ${reading.keys.join(", ")}. A declaration has only ${DECLARATION_KEYS.join(", ")} (FILES §2.6), and the form would drop the rest, so it returns once they are gone.`;
    case "malformed": {
      const path = schemaPathText(reading.error.path);
      return `Outside the declaration grammar at ${path === "" ? "the payload" : path}: ${schemaErrorMessage(reading.error)} (FILES §2.6). The form returns once it parses.`;
    }
  }
}

// The payload of a `Type` node (FILES §2.6), edited structurally and written
// back as the word encoding. A payload the form cannot represent exactly
// (outside the grammar, or with other keys) is edited as JSON.
export function TypeDeclarationEditor({
  payload,
  onChange,
}: {
  payload: Body;
  onChange: (payload: Body) => void;
}) {
  const [raw, setRaw] = useState(false);
  const rawBody = useRawBody(payload, onChange);
  const reading = declarationForForm(payload);
  const showRaw = raw || reading.kind !== "ok";
  return (
    <section className="payload-editor">
      <div className="row">
        <h4>type declaration</h4>
        <ModeToggle
          raw={showRaw}
          disabled={showRaw && (reading.kind !== "ok" || rawBody.blocked)}
          onToggle={() => {
            if (!showRaw) {
              setRaw(true);
            } else if (rawBody.flush()) {
              setRaw(false);
            }
          }}
        />
      </div>
      {reading.kind === "ok" && !showRaw ? (
        <DeclarationForm
          schema={reading.schema}
          named={Object.hasOwn(payload, "name")}
          onChange={(changed) => onChange(declarationBody(changed))}
        />
      ) : (
        <RawBodyEditor
          payload={payload}
          onChange={onChange}
          raw={rawBody}
          // Typing in raw mode keeps it, so applying text that happens to
          // parse does not swap the textarea for the form mid-edit.
          onType={() => setRaw(true)}
          {...(reading.kind === "ok" ? {} : { note: rawNote(reading) ?? "" })}
        />
      )}
    </section>
  );
}
