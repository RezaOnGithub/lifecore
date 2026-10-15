import { useState } from "react";
import type { Format } from "../contract.ts";
import { useDraft } from "../draft/context.tsx";
import { requestOf } from "../draft/model.ts";

// The exported text as of the click; a later edit is flagged, not
// re-encoded. A refusal or failure shows here, never in place of a banner.
export function ExportPanel() {
  const { state, actions } = useDraft();
  const [format, setFormat] = useState<Format>("toml");
  const [exported, setExported] = useState<{
    format: Format;
    text: string;
    request: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    const request = requestOf(state);
    const result = await actions.encode(format);
    if (result.kind === "ok") {
      setExported({ format, text: result.text, request });
      setError(null);
    } else {
      setError(result.message);
    }
  };
  return (
    <section className="export" data-testid="export">
      <div className="row">
        <span className="toggle">
          {(["toml", "json"] as const).map((option) => (
            <button
              key={option}
              type="button"
              className={format === option ? "on" : undefined}
              aria-pressed={format === option}
              onClick={() => setFormat(option)}
            >
              {option.toUpperCase()}
            </button>
          ))}
        </span>
        <button
          type="button"
          disabled={
            state.proposal.entries.length === 0 || state.pending !== null
          }
          onClick={() => void run()}
        >
          Encode
        </button>
      </div>
      {error !== null && (
        <p className="export-error" role="alert" data-testid="export-error">
          {error}
        </p>
      )}
      {exported !== null && (
        <>
          {exported.request !== requestOf(state) && (
            <p className="muted">the draft changed since this export</p>
          )}
          <textarea
            className="export"
            aria-label="exported proposal"
            readOnly
            rows={8}
            value={exported.text}
          />
          <div className="row">
            <button
              type="button"
              onClick={() => void navigator.clipboard.writeText(exported.text)}
            >
              Copy
            </button>
            <a
              download={`draft.${exported.format}`}
              href={`data:text/plain;charset=utf-8,${encodeURIComponent(exported.text)}`}
            >
              Download
            </a>
          </div>
        </>
      )}
    </section>
  );
}
