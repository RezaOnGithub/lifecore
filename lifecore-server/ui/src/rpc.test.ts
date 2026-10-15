import assert from "node:assert/strict";
import { test } from "node:test";
import { response as described } from "./fixtures/database.describe.ts";
import { application } from "./fixtures/errors.ts";
import { rejections } from "./fixtures/http.ts";
import {
  HttpRejectionError,
  RpcClient,
  RpcError,
  TransportError,
} from "./rpc.ts";

interface Sent {
  url: string;
  headers: Record<string, string>;
  body: { jsonrpc: string; id: number; method: string; params: unknown };
}

// A fetch that records each call and answers with `answer`, echoing the
// call's ID into JSON-RPC answers as the server does.
function server(status: number, answer: unknown) {
  const sent: Sent[] = [];
  const fetchImpl = async (url: string | URL | Request, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body));
    sent.push({
      url: String(url),
      headers: init?.headers as Record<string, string>,
      body,
    });
    const reply =
      typeof answer === "object" && answer !== null && "jsonrpc" in answer
        ? { ...answer, id: body.id }
        : answer;
    return new Response(JSON.stringify(reply), { status });
  };
  return { sent, fetchImpl: fetchImpl as typeof fetch };
}

test("a call carries the token and a JSON-RPC envelope and returns the result", async () => {
  const { sent, fetchImpl } = server(200, described);
  const client = new RpcClient("secret", fetchImpl);
  const result = await client.call("database.describe", {});
  assert.deepEqual(result, described.result);
  assert.equal(sent.length, 1);
  assert.equal(sent[0]?.url, "/rpc");
  assert.deepEqual(sent[0]?.headers, {
    "Content-Type": "application/json",
    Authorization: "Bearer secret",
  });
  assert.deepEqual(sent[0]?.body, {
    jsonrpc: "2.0",
    id: 1,
    method: "database.describe",
    params: {},
  });
  await client.call("database.describe", {});
  assert.equal(sent[1]?.body.id, 2);
});

test("without a token no Authorization header is sent", async () => {
  const { sent, fetchImpl } = server(200, described);
  await new RpcClient(null, fetchImpl).call("database.describe", {});
  assert.equal("Authorization" in (sent[0]?.headers ?? {}), false);
});

test("a JSON-RPC error keeps its code, kind, and data", async () => {
  const busy = application.busy[0];
  const { fetchImpl } = server(200, busy);
  const failure = await new RpcClient("secret", fetchImpl)
    .call("proposal.apply", { proposal: { entries: [] } })
    .catch((error: unknown) => error);
  assert.ok(failure instanceof RpcError);
  assert.equal(failure.method, "proposal.apply");
  assert.equal(failure.error.code, 1006);
  assert.ok("data" in failure.error);
  assert.equal(failure.error.data.kind, "busy");
});

test("an HTTP rejection keeps its status and structured body", async () => {
  const { status, body } = rejections.token;
  const { fetchImpl } = server(status, body);
  const failure = await new RpcClient("stale", fetchImpl)
    .call("database.describe", {})
    .catch((error: unknown) => error);
  assert.ok(failure instanceof HttpRejectionError);
  assert.equal(failure.status, 401);
  assert.deepEqual(failure.rejection, body);
});

test("an unstructured HTTP failure still reports its status", async () => {
  const fetchImpl = (async () =>
    new Response("not found", { status: 404 })) as typeof fetch;
  const failure = await new RpcClient("secret", fetchImpl)
    .call("database.describe", {})
    .catch((error: unknown) => error);
  assert.ok(failure instanceof HttpRejectionError);
  assert.equal(failure.rejection, null);
  assert.match(failure.message, /404/);
});

test("a lost connection or a mismatched answer is a transport failure", async () => {
  const unreachable = (async () => {
    throw new TypeError("NetworkError");
  }) as typeof fetch;
  const lost = await new RpcClient("secret", unreachable)
    .call("database.describe", {})
    .catch((error: unknown) => error);
  assert.ok(lost instanceof TransportError);
  const other = (async () =>
    new Response(JSON.stringify({ ...described, id: 99 }), {
      status: 200,
    })) as typeof fetch;
  const mismatched = await new RpcClient("secret", other)
    .call("database.describe", {})
    .catch((error: unknown) => error);
  assert.ok(mismatched instanceof TransportError);
});
