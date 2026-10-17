import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  Dismissal,
  EntryId,
  ErrorKind,
  ErrorObject,
  FindingKey,
} from "../contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import { classifyEdit } from "./dismissalEdits.ts";

const id = (position: string): EntryId => ({ origin: "o", position });

function key(certificate: string): FindingKey {
  return {
    database: "db",
    analyzer: { name: "A", version: "1", configuration: "{}" },
    kind: "warning",
    analyzer_certificate: certificate,
  };
}

const record = (
  certificate: string,
  note: string | null = null,
): Dismissal => ({
  key: key(certificate),
  reason: "acknowledged",
  note,
});

function rpcFailure(kind: ErrorKind, extra: object = {}): RpcError {
  return new RpcError("findings.dismiss", {
    code: 1,
    message: kind,
    data: { kind, ...extra },
  } as unknown as ErrorObject);
}

test("edit failures are classified by what they say about the records", () => {
  const before: ErrorKind[] = [
    "busy",
    "lock_failed",
    "publication_failed",
    "database_missing",
    "database_unreadable",
    "invalid_params",
    "foreign_database",
    "cannot_dismiss_inconsistency",
  ];
  for (const kind of before) {
    assert.equal(classifyEdit(rpcFailure(kind)).kind, "failed", kind);
  }
  const envelope = new RpcError("findings.dismiss", {
    code: -32600,
    message: "invalid request",
  });
  assert.equal(classifyEdit(envelope).kind, "failed");
  assert.equal(
    classifyEdit(
      new HttpRejectionError(403, { kind: "token", message: "no" }, ""),
    ).kind,
    "failed",
  );
  const written = classifyEdit(
    rpcFailure("publication_uncertain", {
      outcome: { kind: "restored", dismissals: [record("a")], restored: true },
    }),
  );
  assert.deepEqual(written.kind === "written" ? written.dismissals : null, [
    record("a"),
  ]);
  const uncertain = [
    rpcFailure("publication_uncertain", {
      outcome: { kind: "initialized", path: "p" },
    }),
    rpcFailure("backend_failure"),
    rpcFailure("internal"),
    rpcFailure("entry_not_found", { id: id("1") }),
    rpcFailure("something_new" as ErrorKind),
    new HttpRejectionError(502, null, "bad gateway"),
    new TransportError("lost"),
    new Error("unreadable"),
  ];
  for (const failure of uncertain) {
    assert.equal(classifyEdit(failure).kind, "uncertain", String(failure));
  }
});
