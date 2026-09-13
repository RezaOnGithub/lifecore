# AGENTS.md

LifeCore is an experimental Rust database with an append-only, typed graph.
The cleanup direction and open decisions are in [docs/PLAN.md](docs/PLAN.md).
`docs/SPEC.md` and `docs/FILES.md` describe proposed contracts, not necessarily
current behavior. Stages 0–2 are complete: the old CLI/sidecars and context
wrappers are removed. Values are scalars, lists, and records. Next are inspection,
analyzer presentation, and TOML authoring. Document collections are backlogged.

## Compatibility

This is a sketch. Backwards compatibility is not a concern at all: APIs, formats,
persisted representations, and commands may change freely within the requested
scope. Do not add compatibility shims, legacy readers, or migration machinery.
Preserve historical correctness under the chosen model and unrelated user work.

## Checks

Active workspace: `lifecore` (root), `lifecore-typecheck`, and
`lifecore-dag-check`. Shared checking and file persistence live in
`lifecore::transaction`; the old CLI and sidecars have been removed.

- Tests: `cargo test --workspace` or `cargo test -p <package>`.
- Formatting: `cargo +nightly fmt --check -p <package>`; omit `--check` to format.
- Lint: `cargo clippy -p <package> --all-targets --no-deps`.

Rust targets stable; rustfmt requires nightly. The active workspace passes
Clippy with `-D warnings`. Keep checks and formatting scoped to the change.

## Invariants

- `LifeCoreList::view()` borrows immutable entries; `View::prefix(end)` only
  narrows the exclusive boundary. Lookups require an `EntryId`, not a position.
- Entry identity is separate from its interpretation in a bounded history view.
  Freshness and active type declarations depend on that view; never inspect a
  future suffix or cache resolved types without the source and boundary.
- Edges retain exact endpoint identities. Superseding an endpoint makes an edge
  stale in views containing the successor; it does not retarget the edge.
- Test history-sensitive changes at multiple boundaries, including before and
  after the relevant append.
- All mutations go through `transaction::check` and a `CheckedPlan` commit.
  `Reference::Added(n)` addresses an earlier operation within that plan;
  `Reference::Existing(id)` retains exact identity across views.
- Core mutation failures leave the receiver unchanged. Preserve atomic composite
  writes and every-prefix checks through shared staging, validation, and
  persistence paths.
- Keep structural validation failures distinct from analyzer diagnostics.
  TypeCheck remains a configured analyzer. Its metatype is fixed and built in;
  no bootstrap node is needed. Empty correctly shaped collections are present.
- Preserve full-prefix analyzer execution, typed inner findings, normalization,
  and third-party/dynamically dispatched implementations. Keep analyzers and
  their private findings/factories together; shared orchestration uses traits.
- Analyzer inspection is read-only and prefix-bounded. Initialization has no
  database access. Results must be deterministic for fixed graph, implementation,
  configuration, and evaluation inputs. Missing configured factories are errors.
- Dismissal and display filtering cannot affect the inconsistency gate.
  Non-emission or supersession alone does not establish resolution.

## Editing conventions

- End every agent-authored commit message with
  `Co-authored-by: Codex <codex@openai.com>`.
- Add agent-authored tests only in a module matching `*::test::test2`.
- Prefix agent-authored Rust comments with `//A`; do not add block comments,
  Rust doc comments, or doctests unless requested. Leave existing comment styles
  alone unless their content needs changing.
- Explain non-obvious invariants, boundary choices, and ordering constraints;
  avoid narrating self-explanatory code.
- Use `//A TODO:` for concrete deferred work and `//A HACK:` for a necessary
  workaround. Explain the impact and next step in the handoff when material.
