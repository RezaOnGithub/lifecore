import type { Body, FieldDecls, Payload, Shape } from "../contract.ts";

// One-line rendering of a payload value for table cells. Time kinds show
// their canonical strings; a variant shows its tag before its body.
export function formatPayload(value: Payload): string {
  if (typeof value === "string") {
    return value;
  }
  if (Array.isArray(value)) {
    return `[${value.map(formatPayload).join(", ")}]`;
  }
  switch (value.kind) {
    case "integer":
    case "timestamp":
    case "duration":
    case "date":
    case "offset_timestamp":
      return value.value;
    case "boolean":
      return value.value ? "true" : "false";
    case "record":
      return formatBody(value.value);
    case "variant":
      return `${value.tag} ${formatBody(value.value)}`;
  }
}

export function formatBody(body: Body): string {
  const members = Object.entries(body).map(
    ([field, value]) => `${field}: ${formatPayload(value)}`,
  );
  return `{${members.join(", ")}}`;
}

export function truncate(text: string, max = 80): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

function declsWords(fields: FieldDecls): string {
  const members = Object.entries(fields).map(
    ([field, decl]) => `${field}: ${shapeWords(decl.shape)}`,
  );
  return `{${members.join(", ")}}`;
}

// Words for a declared shape: "text", "list of text", "record {owner: text}",
// "enum day | week", "variant committed {cost: duration} | resumed {}".
export function shapeWords(shape: Shape): string {
  switch (shape.kind) {
    case "text":
    case "integer":
    case "boolean":
    case "timestamp":
    case "duration":
    case "date":
    case "offset_timestamp":
      return shape.kind;
    case "list":
      return `list of ${shapeWords(shape.element)}`;
    case "record":
      return `record ${declsWords(shape.fields)}`;
    case "enum":
      return `enum ${shape.members.join(" | ")}`;
    case "variant": {
      const tags = Object.entries(shape.tags).map(
        ([tag, fields]) => `${tag} ${declsWords(fields)}`,
      );
      return `variant ${tags.join(" | ")}`;
    }
  }
}
