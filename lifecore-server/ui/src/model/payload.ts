import type { Body, Payload, Shape } from "../contract.ts";

// One-line rendering of a payload value for table cells.
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
      return value.value;
    case "boolean":
      return value.value ? "true" : "false";
    case "record":
      return formatBody(value.value);
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

// Words for a declared shape: "text", "list of text", "record {owner: text}".
export function shapeWords(shape: Shape): string {
  switch (shape.kind) {
    case "text":
    case "integer":
    case "boolean":
    case "timestamp":
      return shape.kind;
    case "list":
      return `list of ${shapeWords(shape.element)}`;
    case "record": {
      const members = Object.entries(shape.fields).map(
        ([field, decl]) => `${field}: ${shapeWords(decl.shape)}`,
      );
      return `record {${members.join(", ")}}`;
    }
  }
}
