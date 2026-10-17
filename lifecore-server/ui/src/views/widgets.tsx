import type { Currentness, TypeResolution } from "../contract.ts";
import { schemaErrorText } from "../model/schema.ts";

export function CurrentnessSelect({
  id,
  value,
  onChange,
  disabled = false,
}: {
  id?: string;
  value: Currentness;
  onChange: (value: Currentness) => void;
  disabled?: boolean;
}) {
  return (
    <select
      id={id}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value as Currentness)}
    >
      <option value="current">current</option>
      <option value="historical">historical</option>
      <option value="all">all</option>
    </select>
  );
}

export function ResolutionBadge({
  resolution,
}: {
  resolution: TypeResolution;
}) {
  switch (resolution.kind) {
    case "builtin":
    case "declared":
      return null;
    case "missing":
      return <span className="badge warn">missing</span>;
    case "ambiguous":
      return <span className="badge warn">ambiguous</span>;
    case "malformed":
      return (
        <span className="badge warn" title={schemaErrorText(resolution.error)}>
          malformed
        </span>
      );
  }
}
