# Stage 10 Release-Readiness Report

Date: 2026-07-26

## Result

Stage 10 implementation and implementor verification are complete. The CLI
package has 55 passing tests, the complete workspace test suite passes, and the
rebuilt binary completed the isolated walkthrough described below.

This report does not claim human review, progression, or the final exit gate.
Those gates remain reviewer-owned or intentionally deferred in `PLAN.md`.

## Implementation summary

- Added test-only `AcceptanceHarness`, `StateSnapshot`, `ExpectedOutput`, JSON
  contract assertions, transaction-envelope assertions, and canonical
  no-`Mock` checks under `test::test2`.
- Added a fresh-state batch scenario covering bootstrap, type creation,
  current and historical type overrides, ordinary appends, an edge, an alias,
  query-attested dry-run and guarded commit, query explanation, Interactive
  Suggestions, dismiss/restore/apply, reload boundaries, and deterministic DOT.
- Added a sidecar scenario covering recursive `Sequence` and `OrderedSet`
  payloads, byte-identical dumps, predicted locators, guarded commit, source
  preservation, reload, and post-commit entry inspection.
- Dismissal-registry output now includes the effective `view` in JSON and human
  presentation.
- `--expect-head` help and SPEC command synopses now consistently advertise
  `ENTRY-LOCATOR|EMPTY` and recommend the guard for automation.
- Removed every production `expect`/`unreachable!` in `lifecore-cli`; malformed
  detail outcomes and entry-field serialization also return ordinary I/O
  errors instead of indexing or panicking.
- Added early structural validation for serialized core storage indexes,
  aggregate ordering/cycles, and `Mock` table invariants. Corrupted stored state
  now returns typed `InvalidState` errors without panic or rewrite.
- Protected canonical state from GraphViz artifact targets, including resolved
  path aliases. DOT artifacts now use same-directory temporary replacement.
- Moved committed transaction result construction before expectation checking
  and save, so no fallible response assembly remains after canonical
  replacement.
- Moved the injected save-failure seam after temporary-file creation and proved
  failure cleanup leaves neither changed canonical bytes nor leaked files.

## Acceptance traceability

The final paragraph of `SPEC.md` contains 20 acceptance clauses.

| # | Acceptance clause | Evidence |
|---|---|---|
| 1 | Dry-run append preserves exact state bytes | `stage6_add_connect_head_and_edge_expectations_use_atomic_transactions`; `stage10_acceptance_harness_spans_canonical_batch_workflows` |
| 2 | Every committed state contains no `Mock` | `stage5_bootstrap_suggestion_dry_run_commit_and_orphan_restore_are_atomic`; both Stage 10 scenarios |
| 3 | Basic-validation and analyzer failures roll back the whole candidate | `stage5_transaction_composites_preserve_order_and_roll_back_every_failure`; `stage5_inconsistencies_block_transactions_and_cannot_be_dismissed` |
| 4 | A correct index with the wrong digest is rejected | `locator_resolution_is_strict_and_checks_in_the_required_order` |
| 5 | Aliases and locators are disjoint; a bad fingerprint never becomes an alias | `node_references_are_lexically_disjoint_and_head_skips_the_latest_edge`; `stage7_aliases_are_exact_rebindable_view_aware_and_byte_safe` |
| 6 | `--select` rejects a locator absent from the query result | `stage7_query_actions_attest_membership_and_fix_the_selected_parent`; Stage 10 batch scenario |
| 7 | `--explain` reports non-membership without selecting it | `stage3_cli_lists_selects_inspects_and_explains_without_writing_state`; Stage 10 batch scenario |
| 8 | Direct type override honors name, view, parent, and `--as` | `stage8_overrides_preserve_order_parent_history_and_query_attestation`; Stage 10 batch scenario |
| 9 | Malformed or duplicate field assignments never append | `stage6_payload_input_converts_recursive_values_and_rejects_every_unsupported_shape`; `stage10_malformed_fields_and_graphviz_state_targets_are_byte_safe` |
| 10 | Nested sequences and ordered sets round-trip through sidecars | `stage10_sidecar_round_trips_recursive_payloads_and_predicted_locators` |
| 11 | A valid sidecar reaches query, entry, type, GraphViz, and `check` | `stage9_ready_sidecar_feeds_every_read_surface_and_commits_atomically`; rebuilt-binary walkthrough |
| 12 | A diagnosable sidecar reaches its final entry while browsing is unavailable | `stage9_diagnosable_check_reaches_final_entry_and_detach_recovers`; rebuilt-binary walkthrough |
| 13 | Dump order is preserved and predicted locators survive commit | `stage9_ready_sidecar_feeds_every_read_surface_and_commits_atomically`; Stage 10 sidecar scenario resolves the predicted locator after reload |
| 14 | An Inconsistency cannot be dismissed | `stage5_inconsistencies_block_transactions_and_cannot_be_dismissed` |
| 15 | A dismissal survives reload | `stage5_dismissal_filters_are_independent_and_keep_message_skips_registration`; Stage 10 invokes a fresh adapter for dismiss, list, and restore |
| 16 | Prefix ambiguity spans visible, dismissed, and resolved messages | `message_prefix_resolution_detects_syntax_unknown_and_ambiguity` now requires a visible/resolved collision plus a dismissal |
| 17 | An unknown analyzer factory is not persisted | `stage6_analyzer_list_and_add_preserve_order_and_reject_missing_factories` |
| 18 | `HEAD` is latest-node-only, rejects an empty graph and stable positions, and resolves per sidecar entry | `node_references_are_lexically_disjoint_and_head_skips_the_latest_edge`; `stage9_planner_preserves_order_head_timing_aliases_and_directions`; parser tests reject `--expect-head HEAD` |
| 19 | `--expect-head` mismatch saves nothing | `stage5_bootstrap_suggestion_dry_run_commit_and_orphan_restore_are_atomic`; Stage 9 rollback test; rebuilt-binary walkthrough |
| 20 | An invalid attached sidecar can detach without being read | `stage9_attach_and_detach_are_metadata_only_and_byte_reversible`; rebuilt-binary malformed/deleted-source recovery |

Additional Stage 10 invariants are covered by:

- root/nested help and removed-interactive tests;
- repeated JSON sidecar dumps and repeated DOT generation;
- stable full and display locators in transaction assertions;
- dismissal `{ "view": ..., "items": [...] }` assertions;
- malformed storage-index cases wrapped with `catch_unwind`;
- atomic-save temporary cleanup assertions;
- protected GraphViz state-target regressions;
- existing stage-specific rollback and sidecar capability matrices.

## Audit findings

### Fixed

1. Dismissal listings omitted their applicable view.
2. Help and command synopses under-described the accepted `EMPTY` head guard.
3. Five production assertion paths and three detail-output indexing paths could
   panic if their construction assumptions were violated.
4. GraphViz could overwrite `LifeCoreState.json` when selected as its target.
5. Raw-deserialized storage indexes and aggregate cycles could panic or
   overflow before producing `InvalidState`.
6. Committed response construction was fallible after state replacement.
7. The save-failure seam did not exercise temporary-file cleanup.

### Confirmed without changes

- `StateStore::load` validates exactly once. The plan's duplicate-validation
  item was stale. Pre-save validation and the fresh compare-before-save load
  protect different boundaries and remain required.
- Query, type, alias, message, transaction-entry, dismissal, and GraphViz
  ordering paths have explicit total or append order.
- Command adapters reuse transaction, analysis, reference, view, effective
  graph, and sidecar services; no private mutation pipeline was found.
- The built-in `Interactive` analyzer remains registered. Remaining
  `interactive` references are analyzer references, rejection tests, or
  explicit future-TTY deferrals.
- Selective `RUST_LOG` output is directed to stderr and does not contaminate
  JSON stdout.

## Rebuilt-binary walkthrough

The actual `target/debug/lifecore-cli` binary was exercised in
`/tmp/lifecore-stage10-walkthrough.2vIkhV`.

- Root and nested help succeeded. Sidecar commit help displayed
  `ENTRY-LOCATOR|EMPTY`.
- `lifecore interactive --format json` exited 2, wrote only stderr, and created
  no state.
- Bootstrap dry-run preserved bytes; guarded application committed entry
  `0:7fd6a21a1225b233`.
- The workflow created and historically branched `Note`, added nodes and an
  edge, bound an alias through query selection, and query-attested a child
  dry-run and guarded commit at `7:916be26e8bb405b1`.
- An Interactive Suggestion `m:44e95891acc20586` was dismissed, listed with its
  view, restored, dry-run, and guarded-applied.
- DOT and SVG artifacts were produced (272 and 4792 bytes).
- `RUST_LOG=lifecore_cli=trace` left parseable JSON on stdout and emitted trace
  records only on stderr.
- A valid sidecar was dumped twice byte-identically, browsed through query,
  entry, type, `check`, and GraphViz, then committed. Predicted locator
  `9:96473ff60f07392e` resolved after reload; source bytes were unchanged.
- An analysis-inconsistent sidecar remained diagnosable while ordinary browsing
  failed. A malformed sidecar and a deleted sidecar source both detached
  successfully.
- An expected-head mismatch and a GraphViz target equal to canonical state both
  exited 1 and preserved canonical SHA-256
  `03be7357d223f21fcb8ba3aba7b5d7b9b3317a5e91b7d4fc3554ab397a510198`.

## Verification

| Check | Result |
|---|---|
| `cargo test -p lifecore-cli` | Pass: 55 tests |
| `cargo test --workspace` | Pass: 35 core, 55 CLI, 22 typecheck, and 1 doctest |
| `cargo +nightly fmt --check -p lifecore-cli` | Pass |
| `cargo clippy -p lifecore-cli --all-targets --no-deps -- -D warnings` | Pass |
| Rebuilt root/nested help and rejected-interactive walkthrough | Pass |
| Repeated JSON and DOT byte comparisons | Pass |
| Selective `RUST_LOG` stdout/stderr separation | Pass |
| `git diff --check` | Pass |
| Complete isolated manual walkthrough | Pass |

## Known warnings

The wider workspace still reports the pre-existing unused
`package.lints` manifest key, unexpected `cfg(revise)`/`cfg(never)` names,
unused core test imports/variables, the unused `AnalysisRunner::analyzer` field,
and unused reference-implementation resolver helpers. The strict
`lifecore-cli` clippy command passes with `-D warnings`; Stage 10 does not widen
into unrelated workspace warning cleanup.

## Gate status

All implementor-owned Stage 10 checklist, automated verification, manual
walkthrough, summary, and implementation-gate items have current evidence and
may be checked. Human review, review gate, progression gate, and final exit gate
remain unchecked.
