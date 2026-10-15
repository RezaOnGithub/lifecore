import { useState } from "react";
import type { Body, TypeSchema } from "../contract.ts";
import { ModeToggle } from "./inputs.tsx";
import { RawBodyEditor, useRawBody } from "./RawJsonEditor.tsx";
import { FieldsEditor } from "./ValueEditor.tsx";

// A node payload: schema-driven rows when the type resolves to a
// declaration, else undeclared rows; or the whole body as JSON text.
export function PayloadEditor({
  payload,
  schema,
  onChange,
}: {
  payload: Body;
  schema: TypeSchema | null;
  onChange: (payload: Body) => void;
}) {
  const [raw, setRaw] = useState(false);
  const rawBody = useRawBody(payload, onChange);
  return (
    <section className="payload-editor">
      <div className="row">
        <h4>payload</h4>
        {schema === null && (
          <span className="muted">
            no declared schema for this type; fields are edited as JSON
          </span>
        )}
        <ModeToggle
          raw={raw}
          disabled={raw && rawBody.blocked}
          onToggle={() => {
            // Text that parses is applied on the way out; text that does
            // not keeps raw mode, since the form could not show it.
            if (!raw || rawBody.flush()) {
              setRaw(!raw);
            }
          }}
        />
      </div>
      {raw ? (
        <RawBodyEditor payload={payload} onChange={onChange} raw={rawBody} />
      ) : (
        <FieldsEditor
          body={payload}
          fields={schema === null ? null : schema.fields}
          prefix={null}
          onChange={onChange}
        />
      )}
    </section>
  );
}
