# Analyzer Guide

Current practice for implementing LifeCore analyzers. Extend this guide as the
contracts become clearer.

## Boundaries

Keep each analyzer, its private message and output types, and its factory in one
module. Shared orchestration should know them only through the core traits.
Analyzers report graph meaning; they do not perform core validation, mutate the
graph, write CLI state, or format user-facing output.

## Contexts

Analyze the `Context` supplied by `AnalysisRunner` and preserve its exclusive
end in every lookup. Use `EntryContext` for inspection and an absolute
`EntryIndex` only for identity comparisons across windows.

Do not derive a total entry count, scan merely to discover one, or use
`LifeCore::total_event_count()`. Needing a graph-wide upper bound is usually a
sign that contextual information has been discarded.

Do not cache resolved types or freshness without their inspection context;
both can change across graph history.

## Messages

Use a typed inner message enum for discovered facts. Convert them into general
`Message` values only at the analyzer boundary.

Diagnostic locations and relevance should carry the contexts and entry
identities that justify the message. Presentation code should not reconstruct
those facts from a separately calculated graph size.

Diagnostics are not validation failures. Report them through analyzer output
rather than rejecting otherwise structurally valid storage.

## Factories and configuration

The factory owns its registered name. Persist and register configurations using
`AnalyzerFactory::analyzer_name()` rather than a duplicate string constant.

Parse and validate analyzer-specific configuration in
`AnalyzerFactory::analyzer_with_config()`. Do not silently substitute another
analyzer when a configured factory is absent; `AnalysisRunner` must return the
missing-factory error.

Use `initialize()` only for analyzer setup and diagnostics that precede
historical-window analysis. Keep `analyze()` deterministic for a given graph,
configuration, and bounded context.
