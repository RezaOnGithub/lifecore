import assert from "node:assert/strict";
import { test } from "node:test";
import type { ErrorKind, ErrorObject, ProposalReport } from "../contract.ts";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import { type Classified, classify } from "./classify.ts";

const report: ProposalReport = {
  database: "db",
  base_revision: "r",
  base: { database: "db", end_exclusive: "1", head: null },
  operations: [],
  analysis: {
    view: { database: "db", end_exclusive: "1", head: null },
    inputs: { evaluation_time: null },
    implementations: [],
    observations: [],
  },
};

function rpc(error: ErrorObject): RpcError {
  return new RpcError("proposal.apply", error);
}

function failure(kind: ErrorKind): RpcError {
  // The shapes of `data` beyond `kind` do not matter to the classification
  // of most kinds; the ones it reads get their members.
  const data: Record<string, unknown> = { kind };
  if (kind === "stale_base" || kind === "foreign_database") {
    data.expected = "a";
    data.actual = "b";
  }
  if (kind === "rejected") {
    data.report = report;
  }
  if (kind === "malformed_proposal") {
    data.decode = { format: "json", message: "bad", path: "entries[1].x" };
  }
  return rpc({ code: 1, message: kind, data } as unknown as ErrorObject);
}

function kindOf(classified: Classified): string {
  return classified.kind === "problem"
    ? `problem:${classified.problem.kind}`
    : classified.kind;
}

test("an apply failure is uncertain unless known to come before publication", () => {
  const before: [ErrorKind, string][] = [
    ["invalid_params", "problem:failed"],
    ["local_time_unknown", "problem:failed"],
    ["malformed_proposal", "problem:malformed_proposal"],
    ["invalid_proposal", "problem:invalid_proposal"],
    ["acknowledgment_unsupported", "problem:failed"],
    ["foreign_database", "problem:foreign_database"],
    ["stale_base", "stale"],
    ["rejected", "rejected"],
    ["analyzer_failure", "problem:failed"],
    ["busy", "problem:busy"],
    ["lock_failed", "problem:failed"],
    ["database_missing", "problem:failed"],
    ["database_unreadable", "problem:failed"],
    ["publication_failed", "problem:failed"],
  ];
  for (const [kind, expected] of before) {
    assert.equal(kindOf(classify(failure(kind), "apply")), expected, kind);
  }
  for (const kind of [
    "publication_uncertain",
    "backend_failure",
    "internal",
    "entry_not_found",
    "something_new",
  ] as ErrorKind[]) {
    const classified = classify(failure(kind), "apply");
    assert.equal(classified.kind, "uncertain", kind);
    assert.equal(classified.kind === "uncertain" && classified.error, kind);
  }
  const outcome = {
    kind: "applied" as const,
    graph_revision: "r2",
    report,
  };
  assert.deepEqual(
    classify(
      rpc({
        code: 1008,
        message: "unsure",
        data: { kind: "publication_uncertain", outcome },
      }),
      "apply",
    ),
    {
      kind: "uncertain",
      error: "publication_uncertain",
      message: "unsure",
      outcome,
    },
  );
  assert.equal(
    kindOf(classify(new TransportError("lost"), "apply")),
    "uncertain",
  );
  assert.equal(kindOf(classify(new TypeError("body"), "apply")), "uncertain");
  // A structured rejection is the access check's, before dispatch; an
  // unexplained refusal is not known to be.
  assert.equal(
    kindOf(
      classify(
        new HttpRejectionError(401, { kind: "token", message: "no" }, ""),
        "apply",
      ),
    ),
    "problem:failed",
  );
  assert.equal(
    kindOf(classify(new HttpRejectionError(502, null, "proxy"), "apply")),
    "uncertain",
  );
  // An envelope error is detected before dispatch.
  assert.equal(
    kindOf(classify(rpc({ code: -32600, message: "envelope" }), "apply")),
    "problem:failed",
  );
});

test("a check never writes, so a lost answer is only retryable", () => {
  for (const lost of [
    failure("backend_failure"),
    failure("internal"),
    new TransportError("lost"),
    new TypeError("body"),
  ]) {
    assert.equal(kindOf(classify(lost, "check")), "problem:retryable");
  }
  assert.equal(kindOf(classify(failure("busy"), "check")), "problem:busy");
  assert.equal(
    kindOf(classify(failure("entry_not_found"), "check")),
    "problem:failed",
  );
  const malformed = classify(failure("malformed_proposal"), "check", [
    "a",
    "b",
  ]);
  assert.equal(
    malformed.kind === "problem" &&
      malformed.problem.kind === "malformed_proposal" &&
      malformed.problem.operation,
    "b",
  );
});
