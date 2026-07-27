# LifeCore CLI Notes

## Analyzer extension boundary

The `lifecore-cli` library does not currently expose an application-level API
for adding an `AnalyzerFactory`. Although core `AnalysisRunner` supports
caller-registered factories, the CLI constructs its own `AnalyzerRegistry`
from the factories compiled into `lifecore-cli`. A downstream `main` therefore
cannot yet register a custom analyzer and then run the normal CLI command
pipeline.

This is a deferred library-interface omission, not a limitation of the core
analyzer traits. Adding a public registry/application builder would require
threading that registry through CLI dispatch and is intentionally deferred
while the CLI is refactoring-averse.

## Analyzer configuration editing

`lifecore analyzer add NAME CONFIG` can append a configured analyzer to the
stored suite after constructing and running the complete candidate suite. The
CLI does not currently provide commands to replace, reorder, or remove an
existing analyzer configuration. Configuration editing remains deferred.

## Adding analyzers for now

Until the library interface is extended, add an analyzer as a factory compiled
into the `lifecore-cli` utility:

1. Add the analyzer crate as a `lifecore-cli` dependency, or place the analyzer
   and factory in a dedicated internal module.
2. Make the factory implement `AnalyzerFactory<LifeCoreList>`.
3. Add it to `AnalyzerRegistry` and register it in
   `AnalyzerRegistry::runner()`. Keep the test-only empty registry empty.
4. For an opt-in analyzer, stop there. Users can configure it with
   `lifecore analyzer add NAME CONFIG`.
5. For a default analyzer, also add its name and a valid default configuration
   in `initial_graph()`.
6. Obtain the persisted analyzer name from
   `AnalyzerFactory::analyzer_name()` rather than duplicating a name constant.
7. Test successful registration, invalid-configuration rollback, and the
   intended opt-in or default suite behavior.

The stock CLI can only execute analyzer configurations whose factories were
compiled into that binary. State containing another analyzer name will fail
analysis with a missing-factory error.
