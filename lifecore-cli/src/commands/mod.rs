use lifecore::{LifeCore, LifeCoreList};

use crate::{
    InvocationContext,
    analysis::{AnalyzerRegistry, initial_graph, run_analysis},
    app::{CommandOutcome, PredicateResult, QueryExplanationRecord, ViewSummary},
    cli::{Command, EntryCommand, MessageCommand, QueryCommand, SelectionActionKind},
    entry::entry_record,
    error::AppError,
    graphviz::{
        GraphvizExporter, GraphvizInput, GraphvizKind, GraphvizOptions, GraphvizTarget, generate,
    },
    locator::{EntryExpectation, EntryLocator, resolve_entry, resolve_index},
    messages::{MessageFilter, MessageUniverse, Severity, plan_suggestion},
    query::{execute_query, explain_query, parse_query},
    state::{DismissalRecord, MESSAGE_ID_VERSION, PersistedState, StatePath, StateStore},
    transaction::{ExpectedHead, TransactionRequest, TransactionService},
    view::{GraphView, ViewSpec},
};

pub fn dispatch(
    command: Command,
    invocation: &InvocationContext,
) -> Result<CommandOutcome, AppError> {
    let operation = command.operation_name();
    let format = command.output_format();

    match command {
    Command::Init(_) => {
        let store = state_store(invocation)?;
        let lifecore = initial_graph();
        let view_end = view_end(&lifecore);
        let registry = AnalyzerRegistry::built_in();
        let report = run_analysis(&lifecore, &registry)?;
        let universe = MessageUniverse::from_analysis(&report, &lifecore, &[])?;

        if universe
            .records()
            .iter()
            .any(|message| message.severity == Severity::Inconsistency)
        {
            return Err(AppError::analysis(
                "initial graph analysis emitted an Inconsistency",
            ));
        }

        let state = PersistedState::new(lifecore);
        let uuid = state.cli.uuid.to_string();
        store.initialize(&state)?;

        Ok(CommandOutcome::Initialized {
            format,
            path: store.path().to_path_buf(),
            uuid,
            view: ViewSummary {
                end: view_end,
                through: None,
                display_through: None,
            },
            messages: universe.visible(),
        })
    }
    Command::Check(args) => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "check")?;

        let report = run_analysis(&state.lifecore, &AnalyzerRegistry::built_in())?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;

        if args.dismissal_ids {
            Ok(CommandOutcome::Dismissals {
                format,
                items: universe.dismissal_statuses(&state.cli.dismissals),
            })
        } else {
            Ok(CommandOutcome::Messages {
                format,
                view: ViewSummary {
                    end: view_end(&state.lifecore),
                    through: None,
                    display_through: None,
                },
                items: universe.filtered(MessageFilter {
                    include_dismissed: args.include_dismissed,
                    only_dismissed: args.only_dismissed,
                    include_resolved: args.include_resolved,
                }),
                detail: false,
            })
        }
    }
    Command::Message {
        command: MessageCommand::Show(args),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "message show")?;

        let report = run_analysis(&state.lifecore, &AnalyzerRegistry::built_in())?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;
        let message = universe.resolve(&args.message_id)?;

        Ok(CommandOutcome::Messages {
            format,
            view: ViewSummary {
                end: view_end(&state.lifecore),
                through: None,
                display_through: None,
            },
            items: vec![message],
            detail: true,
        })
    }
    Command::Message {
        command: MessageCommand::Dismiss(args),
    } => {
        let store = state_store(invocation)?;
        let mut state = store.load()?;
        reject_attached_sidecar(&state, "message dismiss")?;

        let report = run_analysis(&state.lifecore, &AnalyzerRegistry::built_in())?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;
        let message = universe.resolve(&args.message_id)?;
        if message.severity == Severity::Inconsistency {
            return Err(AppError::wrong_message_severity(
                message.display_id,
                message.severity.label(),
                "Warning or Suggestion",
            ));
        }

        insert_dismissal(&mut state.cli.dismissals, message.id.clone());
        store.save(&state)?;

        Ok(CommandOutcome::DismissalChanged {
            format,
            action: "saved".to_string(),
            id: format!("m:{}", message.id),
            emitted: true,
        })
    }
    Command::Message {
        command: MessageCommand::Restore(args),
    } => {
        let store = state_store(invocation)?;
        let mut state = store.load()?;
        reject_attached_sidecar(&state, "message restore")?;

        let report = run_analysis(&state.lifecore, &AnalyzerRegistry::built_in())?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;
        let digest = universe.resolve_with_dismissals(&args.message_id, &state.cli.dismissals)?;
        let emitted = universe
            .records()
            .iter()
            .any(|message| message.id == digest);
        let Ok(index) = state
            .cli
            .dismissals
            .binary_search_by(|record| record.digest.cmp(&digest))
        else {
            return Err(AppError::dismissal_not_found(args.message_id));
        };
        state.cli.dismissals.remove(index);
        store.save(&state)?;

        Ok(CommandOutcome::DismissalChanged {
            format,
            action: "removed".to_string(),
            id: format!("m:{digest}"),
            emitted,
        })
    }
    Command::Message {
        command: MessageCommand::Apply(args),
    } => {
        let expectation = args
            .transaction
            .expect_head
            .as_deref()
            .map(str::parse::<ExpectedHead>)
            .transpose()?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "message apply")?;

        let registry = AnalyzerRegistry::built_in();
        let report = run_analysis(&state.lifecore, &registry)?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;
        let message = universe.resolve(&args.message_id)?;
        let change = plan_suggestion(&message)?;
        let request = TransactionRequest {
            change,
            commit: args.transaction.commit,
            expected_head: expectation,
            dismiss_after_commit: (!args.keep_message).then_some(message.id),
        };
        let result = TransactionService::new(&store, &registry).execute(&state, request)?;

        Ok(CommandOutcome::Transaction { format, result })
    }
    Command::Entry {
        command: EntryCommand::Show(args),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "entry show")?;
        let view = GraphView::latest(&state.lifecore, &state.cli)?;
        let locator = EntryLocator::parse(&args.locator)?;
        let resolved = resolve_entry(
            &state.lifecore,
            &state.cli,
            &locator,
            EntryExpectation::Any,
            Some(view.end_exclusive()),
        )?;
        let item = entry_record(&state.lifecore, &state.cli, &view, &resolved)?;

        Ok(CommandOutcome::Entries {
            format,
            view: view.summary(),
            items: vec![item],
            detail: true,
        })
    }
    Command::Query {
        command: QueryCommand::Nodes(args),
    } => {
        if args.selection_action_kind().map_err(AppError::usage)?
            == Some(SelectionActionKind::Mutation)
        {
            return Err(AppError::unavailable(operation, format));
        }

        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "query nodes")?;
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let view = GraphView::construct(&state.lifecore, &state.cli, view_spec)?;
        let query = parse_query(&view, args.combinator, &args.predicates)?;

        if let Some(locator_text) = args.explain.as_deref() {
            let locator = EntryLocator::parse(locator_text)?;
            let resolved = resolve_entry(
                &state.lifecore,
                &state.cli,
                &locator,
                EntryExpectation::Node,
                Some(view.end_exclusive()),
            )?;
            let candidate = view
                .context()
                .look_at(resolved.entry.entry_context().absolute());
            let explanation = explain_query(&state.lifecore, &query, candidate)?;
            let candidate = entry_record(&state.lifecore, &state.cli, &view, &resolved)?;
            let predicates = query
                .predicates
                .iter()
                .cloned()
                .zip(explanation.predicate_matches)
                .map(|(predicate, matches)| PredicateResult { predicate, matches })
                .collect();

            return Ok(CommandOutcome::QueryExplanation {
                format,
                view: view.summary(),
                explanation: Box::new(QueryExplanationRecord {
                    candidate,
                    combinator: combinator_name(query.combinator).to_string(),
                    predicates,
                    matches: explanation.matches,
                }),
            });
        }

        let results = execute_query(&state.lifecore, &query);
        if let Some(locator_text) = args.select.as_deref() {
            let locator = EntryLocator::parse(locator_text)?;
            let resolved = resolve_entry(
                &state.lifecore,
                &state.cli,
                &locator,
                EntryExpectation::Node,
                Some(view.end_exclusive()),
            )?;
            if !results
                .iter()
                .any(|node| node.e.entry().0 == resolved.index())
            {
                return Err(AppError::query_selection(locator_text));
            }

            let item = entry_record(&state.lifecore, &state.cli, &view, &resolved)?;

            return Ok(CommandOutcome::Entries {
                format,
                view: view.summary(),
                items: vec![item],
                detail: true,
            });
        }

        let items = results
            .into_iter()
            .map(|node| {
                resolve_index(
                    &state.lifecore,
                    &state.cli,
                    node.e.entry().0,
                    EntryExpectation::Node,
                    Some(view.end_exclusive()),
                )
                .and_then(|resolved| entry_record(&state.lifecore, &state.cli, &view, &resolved))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CommandOutcome::Entries {
            format,
            view: view.summary(),
            items,
            detail: false,
        })
    }
    Command::Graphviz(args) => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "graphviz")?;
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let view = GraphView::construct(&state.lifecore, &state.cli, view_spec)?;
        let options = GraphvizOptions {
            full: args.full,
            fields: args.fields,
        };
        let document = generate(
            GraphvizInput {
                graph: &state.lifecore,
                state: &state.cli,
                view: &view,
            },
            options,
        )?;
        let kind = if args.dot {
            GraphvizKind::Dot
        } else {
            GraphvizKind::Svg
        };
        let target = GraphvizTarget::resolve(args.target, invocation.current_directory(), kind)?;
        let exporter = GraphvizExporter::default();

        match kind {
        GraphvizKind::Dot => exporter.write_dot(&target, &document)?,
        GraphvizKind::Svg => exporter.write_svg(&target, &document)?,
        }

        Ok(CommandOutcome::GraphvizArtifact {
            target: target.path().to_path_buf(),
            artifact_kind: target.kind().name().to_string(),
            view: view.summary(),
        })
    }
    _ => {
        log::trace!("operation '{operation}' is unavailable in stage 5");

        Err(AppError::unavailable(operation, format))
    }
    }
}

fn combinator_name(combinator: crate::cli::QueryCombinator) -> &'static str {
    match combinator {
    crate::cli::QueryCombinator::All => "all",
    crate::cli::QueryCombinator::Any => "any",
    crate::cli::QueryCombinator::None => "none",
    }
}

fn view_end(lifecore: &LifeCoreList) -> usize {
    lifecore
        .last()
        .map_or(0, |entry| entry.entry().0.saturating_add(1))
}

fn state_store(invocation: &InvocationContext) -> Result<StateStore, AppError> {
    let path = StatePath::resolve(
        invocation.state_path_environment.clone(),
        &invocation.current_directory,
    )?;

    Ok(StateStore::new(path))
}

fn reject_attached_sidecar(state: &PersistedState, operation: &str) -> Result<(), AppError> {
    if state.cli.sidecar_path.is_some() {
        return Err(AppError::unavailable(
            format!("{operation} with attached sidecar"),
            crate::cli::OutputFormat::Lines,
        ));
    }

    Ok(())
}

fn insert_dismissal(dismissals: &mut Vec<DismissalRecord>, digest: String) {
    match dismissals.binary_search_by(|record| record.digest.cmp(&digest)) {
    Ok(_) => {}
    Err(index) => dismissals.insert(
        index,
        DismissalRecord {
            version: MESSAGE_ID_VERSION,
            digest,
        },
    ),
    }
}
