import assert from "node:assert/strict";
import { test } from "node:test";
import { TOKEN_KEY, takeLaunchToken } from "./token.ts";

function sources(hash: string, stored: string | null = null) {
  const storage = new Map<string, string>();
  if (stored !== null) {
    storage.set(TOKEN_KEY, stored);
  }
  const replaced: string[] = [];
  return {
    storage,
    replaced,
    sources: {
      location: { hash, pathname: "/", search: "?view=x" },
      history: {
        replaceState: (
          _data: unknown,
          _unused: string,
          url?: string | URL | null,
        ) => {
          replaced.push(String(url));
        },
      },
      storage: {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => {
          storage.set(key, value);
        },
      },
    },
  };
}

test("a fragment token is stored and removed from the address", () => {
  const { storage, replaced, sources: from } = sources("#token=abc123");
  assert.equal(takeLaunchToken(from), "abc123");
  assert.equal(storage.get(TOKEN_KEY), "abc123");
  assert.deepEqual(replaced, ["/?view=x"]);
});

test("other fragment members survive the removal", () => {
  const { replaced, sources: from } = sources("#token=abc&tab=2");
  assert.equal(takeLaunchToken(from), "abc");
  assert.deepEqual(replaced, ["/?view=x#tab=2"]);
});

test("a reload without a fragment uses the stored token", () => {
  const { replaced, sources: from } = sources("", "stored");
  assert.equal(takeLaunchToken(from), "stored");
  assert.deepEqual(replaced, []);
});

test("a new launch URL replaces the stored token", () => {
  const { storage, sources: from } = sources("#token=fresh", "stale");
  assert.equal(takeLaunchToken(from), "fresh");
  assert.equal(storage.get(TOKEN_KEY), "fresh");
});

test("without a fragment or stored token there is none", () => {
  const { sources: from } = sources("#tab=2");
  assert.equal(takeLaunchToken(from), null);
});
