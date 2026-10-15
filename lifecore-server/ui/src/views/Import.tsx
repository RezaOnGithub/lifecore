import { useState } from "react";
import type { Format } from "../contract.ts";
import type { Decoded } from "../draft/actions.ts";
import { useDraft } from "../draft/context.tsx";
import { useOpen } from "../links.tsx";

// The text is sent as typed or read from the file: bare integers in a change
// file are decoded by the Rust codec, never by JSON.parse here.
export function ImportView() {
  const { state, actions } = useDraft();
  const open = useOpen();
  const [text, setText] = useState("");
  const [format, setFormat] = useState<Format>("toml");
  const [decoded, setDecoded] = useState<Decoded | null>(null);
  const [appended, setAppended] = useState<number | null>(null);
  const readFile = async (file: File | undefined) => {
    if (file === undefined) {
      return;
    }
    setText(await file.text());
    setDecoded(null);
    if (file.name.endsWith(".json")) {
      setFormat("json");
    } else if (file.name.endsWith(".toml")) {
      setFormat("toml");
    }
  };
  const decode = async () => {
    setAppended(null);
    setDecoded(await actions.decode(text, format));
  };
  const use = (mode: "replace" | "append") => {
    if (decoded?.kind !== "ok") {
      return;
    }
    const first = decoded.proposal.entries[0];
    actions.importProposal(decoded.proposal, mode);
    setDecoded(null);
    if (mode === "replace" && first !== undefined) {
      open({ kind: "operation", name: first.name });
    } else {
      setAppended(decoded.proposal.entries.length);
    }
  };
  return (
    <section className="importer" data-testid="import">
      <h2>Import a change file</h2>
      <p className="muted">
        JSON or TOML as for the CLI (FILES §2). Bare integers are accepted and
        come back as decimal strings.
      </p>
      <div className="row">
        <label>
          format{" "}
          <select
            aria-label="format"
            value={format}
            onChange={(event) => setFormat(event.target.value as Format)}
          >
            <option value="toml">toml</option>
            <option value="json">json</option>
          </select>
        </label>
        <label>
          file{" "}
          <input
            type="file"
            aria-label="file"
            accept=".json,.toml,application/json,text/plain"
            onChange={(event) => void readFile(event.target.files?.[0])}
          />
        </label>
      </div>
      <textarea
        aria-label="proposal text"
        rows={16}
        cols={90}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          setDecoded(null);
        }}
      />
      <div className="row">
        <button
          type="button"
          disabled={text.trim() === "" || state.pending !== null}
          onClick={() => void decode()}
        >
          Decode
        </button>
        {state.pending === "decode" && <span className="muted">decoding…</span>}
        {appended !== null && (
          <span className="muted" data-testid="appended">
            appended {appended} operation{appended === 1 ? "" : "s"}
          </span>
        )}
      </div>
      {decoded?.kind === "ok" && (
        <div className="banner" data-testid="decoded">
          Decoded {decoded.proposal.entries.length} operation
          {decoded.proposal.entries.length === 1 ? "" : "s"}
          {decoded.proposal.plan_id !== undefined && " with a plan ID"}
          {decoded.proposal.acknowledges !== undefined &&
            " and an acknowledgment"}
          :{" "}
          <code>
            {decoded.proposal.entries.map((entry) => entry.name).join(", ")}
          </code>
          <div className="row">
            <button type="button" onClick={() => use("replace")}>
              Replace draft
            </button>
            <button type="button" onClick={() => use("append")}>
              Append operations
            </button>
          </div>
        </div>
      )}
      {decoded?.kind === "malformed" && (
        <div className="banner problem" role="alert">
          Malformed {decoded.decode.format}: {decoded.decode.message}
          {decoded.decode.path && (
            <>
              {" "}
              at <code>{decoded.decode.path}</code>
            </>
          )}
          {decoded.decode.line !== undefined && (
            <span className="muted">
              {" "}
              (line {decoded.decode.line}
              {decoded.decode.column !== undefined &&
                `, column ${decoded.decode.column}`}
              )
            </span>
          )}
        </div>
      )}
      {decoded?.kind === "failed" && (
        <div className="banner problem" role="alert">
          {decoded.message}
        </div>
      )}
    </section>
  );
}
