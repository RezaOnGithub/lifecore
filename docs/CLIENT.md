# Browser client

Status: contract for the React client in `lifecore-server/ui`. [UI](UI.md)
records its presentation decisions.

## 1. Responsibilities

The client targets a local user and one database. Serve the client and RPC
on the same origin, behind the access checks of
[INTERFACES §4](INTERFACES.md#4-server-access).
The client owns editors, drafts, navigation, and presentation state, including
the grouping of observations by complete finding key with the core's semantics
([ANALYZERS §3](ANALYZERS.md#3-presentation-and-dismissal)). The backend owns
resolution, queries, validation, analysis, Graphviz rendering, and publication.

## 2. Contract and fixtures

Write the TypeScript definitions of the shared contract by hand in
`ui/src/contract.ts`. The contract is small, its methods are reshaped by the
stages, and the types that matter most have hand-written `Serialize`
implementations that a generator could not follow. Guard it with fixtures: a
Rust test in `lifecore-server` records representative requests and responses of
every method, every application error kind, the envelope errors, and every HTTP
rejection, mostly from real calls through the server's own request path, and
commits them under `ui/src/fixtures/`. Each fixture is a TypeScript module that
exports the Rust-serialized JSON value followed by `satisfies` its declared
type, so the frontend type-check rejects a changed Rust shape: a missing or
renamed member, a member the contract does not declare, or a wrong tag.
Imported `.json` files would not do, because `tsc` widens their string literals
and skips excess-member checks. A generated `index.ts` maps every method of the
dispatcher's single method list (`rpc::Method::ALL`) to its fixture, so a method
the contract declares without a fixture, or the reverse, also fails `tsc`.
HTTP rejection statuses are pinned per kind as literal types. Random UUIDs and
scratch paths are replaced by stable stand-ins, so an unchanged shape rewrites
nothing. The same test pins the client's own readers of Rust formats: it
records declaration payloads with `Type::from_body`'s reading, covering every
schema error kind and the depth limit, and node payload texts with the codec's
canonical reading or refusal, and `npm test` asserts that the client reads them
alike. Each payload text records where the raw payload editor differs by
design: it refuses the bare integer shorthand, and it keeps timestamp text as
typed for Jiff to judge.

The fixture test writes `ui/src/fixtures/` when Rust tests run, which is after
`build.rs` has bundled, and fails when the committed copies are stale, so the
build only reads committed files. It rewrites only stale files, so an
unchanged run leaves `ui/src` untouched and does not trigger a rebundle.

## 3. Values

Canonical integer payloads are
`{"kind":"integer","value":"9223372036854775807"}`-shaped objects. Keep the decimal
strings in model state, including inside lists/records, and use `bigint` only
when arithmetic is needed; serialize back to strings. Known large control
integers, such as entry positions and view boundaries, are also decimal strings
but need no payload wrapper. Small bounded protocol fields such as error codes
can remain JSON numbers. Canonical responses can use ordinary JSON parsing; no
lossless-number JSON parser is required. Send raw JSON/TOML file imports to the
Rust codec, which accepts bare `i64` shorthand and returns canonical envelopes.
Never parse that shorthand through JavaScript numbers before sending it.
Keep timestamp strings at full precision in model state; date widgets must not
silently replace them with lower-precision values. The same requirement applies
when importing and exporting change files.

## 4. Drafts, suggestions, and evaluation time

Drafts carry a service-minted `plan_id`. Preserve exact existing references and
stable local operation names. Preview identities are provisional; use returned
operation-to-entry mappings after application. A stale proposal requires
deliberate replanning, without silently refreshing its token.

Suggestion selection is not built
([PLAN §2.2](PLAN.md#22-suggestions-and-acknowledgment)). When it is, selecting
a suggestion selects its ready-made proposal, shown and applied as
[SUGGESTIONS](SUGGESTIONS.md) describes; editing its operations requires
removing `acknowledges`, turning it into an ordinary draft.

Evaluation time is an explicit choice, absent by default; for `now`, the host
captures one instant and reports it ([RPC §1](RPC.md#1-conventions)). Check,
the Findings screen, and analyzer adds each offer their own choice: absent,
`now`, or an explicit timestamp. Each displayed analysis identifies its view,
implementations, configuration strings, and evaluation inputs. Keep
certificates opaque; use supplied subjects and the focus entry for UI
navigation. Dismissal and display filtering never change acceptance.

## 5. Prototype scope

The prototype covers browsing types, entries, lineage, and relationships at any
boundary; Explore queries shown as tables, threads, or Graphviz drawings; drafts
built by form, raw JSON, or import, with check, explicit apply, and
reconnection after revisions; findings with current/history selection, sticky
historical labels, dismissal reasons and notes, and restore; and appending
analyzers. Suggestion selection, comparison between boundaries, and refine
steps are not built ([PLAN §2](PLAN.md#2-remaining-work)).

## 6. Build and tests

The client lives in `lifecore-server/ui` as a small Node project: npm with a
committed `package-lock.json`, esbuild for bundling, `tsc --noEmit` for
type-checking, Biome for linting and formatting, `node --test` for unit tests,
and Playwright Test for browser tests. Its only runtime dependencies are React
and ReactDOM; add libraries only when a stage needs them.

`lifecore-server`'s `build.rs` bundles `ui/src/main.tsx` with the installed
esbuild into a fixed-name `app.js` in `OUT_DIR` and copies the handwritten
`ui/index.html` beside it; the server embeds both. Both steps live in
`ui/build.mjs`, which `build.rs` runs with `node` and the npm `build` and
`watch` scripts run for `ui/dist`, so every bundle uses the same options.
`build.rs` declares `rerun-if-changed` for `ui/src`, `ui/index.html`,
`ui/package-lock.json`, and the inputs that shape the bundle besides them
(`ui/package.json`, `ui/build.mjs`, `ui/tsconfig.json`), and
`rerun-if-env-changed` for `LIFECORE_UI`. It never installs packages: when
`ui/node_modules` is missing, it fails with an instruction to run `npm ci` in
`lifecore-server/ui`. Setting `LIFECORE_UI=skip` embeds a placeholder page
instead, for builds that do not touch the client, such as agents and CI running
`cargo test --workspace` without Node. Type-checking, linting, and tests are
separate checks, not part of the build.

For quick iteration, `serve --ui-dir <dir>` serves `index.html` and `app.js`
from `<dir>`, reading them on every request, while `npm run watch` rebuilds
them into `ui/dist`; a browser refresh picks up changes, and the launch token
survives in session storage.

Browser tests run in Firefox, the priority for compatibility; other browsers are
[backlogged](PLAN.md#3-backlog). A global setup builds `lifecore`
with Cargo, unless `LIFECORE_BIN` names a binary, and refuses a binary that
embeds the `LIFECORE_UI=skip` placeholder. It initializes one scratch database,
with the default TypeCheck suite, through `LIFECORE_STATE_FILE_PATH`, and
starts one `lifecore serve` on it. Tests open the URL it printed, launch token
included, so they exercise the real access flow and the embedded client.

All spec files share that server and database, so Playwright runs one worker:
files run one at a time in name order, and each file runs its tests serially,
mostly on one shared page. A file that needs data applies its own types and
entries through `proposal.apply` over RPC before its tests and leaves other
files' types alone. `zz-analyzers.spec.ts` is named to run last, because the
analyzer it appends changes the suite for every later file. Tests simulate lost
or unconfirmed answers by intercepting `/rpc`.

Graphviz is optional. The rendering test in `navigate.spec.ts` skips when
`dot` is not installed. The two `lifecore-app` tests that run the real `dot`
are ignored by default; `cargo test -p lifecore-app -- --ignored` runs them.
