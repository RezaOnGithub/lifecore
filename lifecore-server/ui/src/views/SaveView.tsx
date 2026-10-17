import { type KeyboardEvent, useState } from "react";
import type { EntryId } from "../contract.ts";
import { usePlaces } from "../data.tsx";
import { type ExploreForm, suggestedName, toParams } from "../model/query.ts";

export function SaveView({
  form,
  label,
}: {
  form: ExploreForm;
  label: (id: EntryId) => string | undefined;
}) {
  const { places, saveView } = usePlaces();
  const [name, setName] = useState<string | null>(null);
  if (name === null) {
    return (
      <button
        type="button"
        disabled={toParams(form) === null}
        title="keep this form in the sidebar under a name"
        onClick={() => setName(suggestedName(form, label))}
      >
        Save view
      </button>
    );
  }
  const trimmed = name.trim();
  const exists = places.views.some((view) => view.name === trimmed);
  const save = () => {
    if (trimmed !== "") {
      saveView(trimmed, form);
      setName(null);
    }
  };
  // Enter saves rather than submitting the Explore form around it.
  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save();
    } else if (event.key === "Escape") {
      setName(null);
    }
  };
  return (
    <span className="save-view">
      <input
        aria-label="view name"
        value={name}
        size={24}
        // biome-ignore lint/a11y/noAutofocus: the field opens on request
        autoFocus
        onChange={(event) => setName(event.target.value)}
        onKeyDown={onKey}
      />
      <button type="button" disabled={trimmed === ""} onClick={save}>
        {exists ? "Replace" : "Save"}
      </button>
      <button type="button" onClick={() => setName(null)}>
        Cancel
      </button>
    </span>
  );
}
