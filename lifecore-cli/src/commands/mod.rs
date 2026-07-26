use lifecore::{Body, EntryIndex, LifeCore, LifeCoreList};

use crate::{
    InvocationContext,
    analysis::{AnalyzerRegistry, initial_graph, run_analysis},
    app::{
        AliasChangeRecord, AliasRecord, AnalyzerRecord, CommandOutcome, PredicateResult,
        QueryExplanationRecord, ViewSummary,
    },
    cli::{
        AliasCommand, AnalyzerCommand, Command, EntryCommand, MessageCommand, QueryCommand,
        SelectionAction, SelectionAliasCommand, SelectionEntryCommand, SelectionTypeCommand,
        SidecarCommand, TransactionArgs, TypeCommand,
    },
    effective::{EffectiveCapability, EffectiveGraph, prepare_effective, reject_attached_mutation},
    error::AppError,
    graphviz::{
        GraphvizExporter, GraphvizInput, GraphvizKind, GraphvizOptions, GraphvizTarget, generate,
    },
    locator::{EntryExpectation, EntryLocator, ResolvedEntry, resolve_entry, resolve_index},
    messages::{MessageFilter, MessageUniverse, Severity, plan_suggestion},
    payload_input::parse_field_assignments,
    query::{execute_query, explain_query, parse_query},
    reference::{AliasName, NodeReference, alias_status, resolve_node_reference, visible_aliases},
    sidecar::{attach_path, detach},
    state::{DismissalRecord, MESSAGE_ID_VERSION, PersistedState, StatePath, StateStore},
    transaction::{ExpectedHead, TransactionRequest, TransactionService},
    type_edit::{
        TypeCreateRequest, TypePatch, inspect_type, list_types, plan_type_append,
        resolve_active_type, resolve_selected_type,
    },
    view::{GraphView, ViewSpec},
};

struct AddRequest {
    type_name: String,
    body: Body,
    parent: Option<EntryIndex>,
}

impl AddRequest {
    fn into_plan(self) -> crate::transaction::PlannedChange {
        crate::transaction::PlannedChange::AddNode {
            type_name: self.type_name,
            body: self.body,
            parent: self.parent,
        }
    }
}

struct ConnectRequest {
    from: EntryIndex,
    type_name: String,
    to: EntryIndex,
}

impl ConnectRequest {
    fn into_plan(self) -> crate::transaction::PlannedChange {
        crate::transaction::PlannedChange::AddEdge {
            from: self.from,
            type_name: self.type_name,
            to: self.to,
        }
    }
}

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
                canonical_end: view_end,
                through: None,
                display_through: None,
            },
            messages: universe.visible(),
        })
    }
    Command::Sidecar {
        command: SidecarCommand::Attach(args),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let path = attach_path(&store, &state, &args.path, invocation.current_directory())?;

        Ok(CommandOutcome::SidecarAttachment {
            action: "attached".to_string(),
            path,
        })
    }
    Command::Sidecar {
        command: SidecarCommand::Detach,
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let path = detach(&store, &state)?;

        Ok(CommandOutcome::SidecarAttachment {
            action: "detached".to_string(),
            path,
        })
    }
    Command::Sidecar {
        command: SidecarCommand::Dump(_),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Dump, "sidecar dump")?;
        let items = effective.dump_suffix()?;
        let view = effective.view(ViewSpec::Latest)?;

        Ok(CommandOutcome::SidecarDump {
            format,
            view: view.summary(),
            items,
        })
    }
    Command::Sidecar {
        command: SidecarCommand::Commit(args),
    } => {
        let expected_head = args
            .expect_head
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map(ExpectedHead::Entry);
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective = effective_graph(
            &state,
            &registry,
            EffectiveCapability::Commit,
            "sidecar commit",
        )?;
        let plan = effective
            .plan()
            .cloned()
            .ok_or_else(AppError::sidecar_not_attached)?;
        let result = TransactionService::new(&store, &registry).execute_sidecar(
            &state,
            plan.change,
            plan.temporary_aliases,
            expected_head,
        )?;

        Ok(CommandOutcome::SidecarCommitted { format, result })
    }
    Command::Add(args) => {
        let expected_head = parse_expected_head(&args.transaction)?;
        let body = parse_field_assignments(&args.fields)?;
        let parent = args
            .parent
            .as_deref()
            .map(NodeReference::parse)
            .transpose()?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_mutation(&state, "add")?;
        let view = GraphView::latest(&state.lifecore, &state.cli)?;
        let parent = parent
            .as_ref()
            .map(|reference| resolve_node_reference(&state.lifecore, &state.cli, reference, &view))
            .transpose()?
            .map(|resolved| EntryIndex(resolved.index()));
        let request = AddRequest {
            type_name: args.r#type,
            body,
            parent,
        };

        log::debug!(
            "prepared node append (parent={}, fields={})",
            request.parent.is_some(),
            request.body.iter().count()
        );
        execute_transaction(
            &store,
            &state,
            request.into_plan(),
            &args.transaction,
            expected_head,
            format,
        )
    }
    Command::Connect(args) => {
        let expected_head = parse_expected_head(&args.transaction)?;
        let from = NodeReference::parse(&args.from)?;
        let to = NodeReference::parse(&args.to)?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_mutation(&state, "connect")?;
        let view = GraphView::latest(&state.lifecore, &state.cli)?;
        let from = resolve_node_reference(&state.lifecore, &state.cli, &from, &view)?;
        let to = resolve_node_reference(&state.lifecore, &state.cli, &to, &view)?;
        let request = ConnectRequest {
            from: EntryIndex(from.index()),
            type_name: args.r#type,
            to: EntryIndex(to.index()),
        };

        log::debug!(
            "prepared edge append (from={}, to={})",
            request.from.0,
            request.to.0
        );
        execute_transaction(
            &store,
            &state,
            request.into_plan(),
            &args.transaction,
            expected_head,
            format,
        )
    }
    Command::Analyzer {
        command: AnalyzerCommand::List(_),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective = effective_graph(
            &state,
            &registry,
            EffectiveCapability::Browse,
            "analyzer list",
        )?;
        let view = effective.view(ViewSpec::Latest)?;
        let items = effective
            .state()
            .lifecore
            .current_analysis_suite()
            .0
            .into_iter()
            .map(|config| AnalyzerRecord {
                name: config.name,
                config: config.config,
            })
            .collect();

        Ok(CommandOutcome::Analyzers {
            format,
            view: view.summary(),
            items,
        })
    }
    Command::Alias {
        command: AliasCommand::List(args),
    } => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Browse, "alias list")?;
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let view = effective.view(view_spec)?;
        let items = effective
            .state()
            .cli
            .aliases
            .iter()
            .map(|(name, target)| {
                let effective_state = effective.state();
                let (resolved, status) = alias_status(
                    &effective_state.lifecore,
                    &effective_state.cli,
                    target,
                    &view,
                )?;

                Ok(AliasRecord {
                    name: name.clone(),
                    locator: resolved.full_locator(),
                    display_locator: resolved.display_locator(),
                    status,
                    temporary: effective.is_temporary_alias(name),
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;

        Ok(CommandOutcome::Aliases {
            format,
            view: view.summary(),
            items,
        })
    }
    Command::Alias {
        command: AliasCommand::Set(args),
    } => {
        let store = state_store(invocation)?;
        let mut state = store.load()?;
        reject_attached_mutation(&state, "alias set")?;
        let view = GraphView::latest(&state.lifecore, &state.cli)?;
        let target = resolve_entry(
            &state.lifecore,
            &state.cli,
            &args.locator,
            EntryExpectation::Node,
            Some(view.end_exclusive()),
        )?;
        bind_alias(&store, &mut state, args.name, &target, format)
    }
    Command::Alias {
        command: AliasCommand::Remove(args),
    } => {
        let store = state_store(invocation)?;
        let mut state = store.load()?;
        reject_attached_mutation(&state, "alias remove")?;
        let target = state.cli.aliases.remove_name(&args.name).ok_or_else(|| {
            AppError::invalid_node_reference(format!("unknown alias {:?}", args.name.as_str()))
        })?;
        let locator = format!("{}:{}", target.index, target.digest);
        store.save(&state)?;

        Ok(CommandOutcome::AliasChanged {
            format,
            alias: AliasChangeRecord {
                action: "removed".to_string(),
                name: args.name.to_string(),
                locator: Some(locator),
            },
        })
    }
    Command::Type {
        command: TypeCommand::List(args),
    } => {
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Browse, "type list")?;
        let view = effective.view(view_spec)?;
        let effective_state = effective.state();
        let items = list_types(&effective_state.lifecore, &effective_state.cli, &view)?
            .into_iter()
            .map(|listing| listing.record())
            .collect();

        Ok(CommandOutcome::Types {
            format,
            view: view.summary(),
            items,
            detail: false,
        })
    }
    Command::Type {
        command: TypeCommand::Show(args),
    } => {
        let locator = EntryLocator::parse(&args.locator)?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Browse, "type show")?;
        let view = effective.view(ViewSpec::Latest)?;
        let effective_state = effective.state();
        let entry = resolve_entry(
            &effective_state.lifecore,
            &effective_state.cli,
            &locator,
            EntryExpectation::Node,
            Some(view.end_exclusive()),
        )?;
        let declaration = inspect_type(&effective_state.lifecore, &view, entry)?;

        Ok(CommandOutcome::Types {
            format,
            view: view.summary(),
            items: vec![declaration.record()],
            detail: true,
        })
    }
    Command::Type {
        command: TypeCommand::Create(args),
    } => {
        let expected_head = parse_expected_head(&args.transaction)?;
        let declaration = TypeCreateRequest::parse(args.name, &args.declaration)?.build();
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_mutation(&state, "type create")?;

        log::debug!("prepared stored type declaration append");
        execute_transaction(
            &store,
            &state,
            plan_type_append(declaration, None),
            &args.transaction,
            expected_head,
            format,
        )
    }
    Command::Type {
        command: TypeCommand::Override(args),
    } => {
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let expected_head = parse_expected_head(&args.transaction)?;
        let patch = TypePatch::parse(args.new_name, &args.patch)?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_mutation(&state, "type override")?;
        let view = GraphView::construct(&state.lifecore, &state.cli, view_spec)?;
        let source = resolve_active_type(&state.lifecore, &state.cli, &view, &args.name)?;
        let parent = EntryIndex(source.entry.index());
        let declaration = patch.apply(&source.declaration)?;

        log::debug!(
            "prepared stored type declaration override (parent={})",
            parent.0
        );
        execute_transaction(
            &store,
            &state,
            plan_type_append(declaration, Some(parent)),
            &args.transaction,
            expected_head,
            format,
        )
    }
    Command::Analyzer {
        command: AnalyzerCommand::Add(args),
    } => {
        let expected_head = parse_expected_head(&args.transaction)?;
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_mutation(&state, "analyzer add")?;
        let change = crate::transaction::PlannedChange::AddAnalyzer {
            name: args.name,
            config: args.config,
        };

        log::debug!("prepared analyzer-suite extension");
        execute_transaction(
            &store,
            &state,
            change,
            &args.transaction,
            expected_head,
            format,
        )
    }
    Command::Check(args) => {
        let store = state_store(invocation)?;
        let state = store.load()?;
        let registry = AnalyzerRegistry::built_in();
        let effective = effective_graph(&state, &registry, EffectiveCapability::Check, "check")?;
        let universe = effective.messages(&registry)?;
        let effective_state = effective.state();
        let view = effective.view(ViewSpec::Latest)?;

        if args.dismissal_ids {
            Ok(CommandOutcome::Dismissals {
                format,
                items: universe.dismissal_statuses(&effective_state.cli.dismissals),
            })
        } else {
            Ok(CommandOutcome::Messages {
                format,
                view: view.summary(),
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
        let registry = AnalyzerRegistry::built_in();
        let effective = effective_graph(
            &state,
            &registry,
            EffectiveCapability::Browse,
            "message show",
        )?;
        let universe = effective.messages(&registry)?;
        let view = effective.view(ViewSpec::Latest)?;
        let message = universe.resolve(&args.message_id)?;

        Ok(CommandOutcome::Messages {
            format,
            view: view.summary(),
            items: vec![message],
            detail: true,
        })
    }
    Command::Message {
        command: MessageCommand::Dismiss(args),
    } => {
        let store = state_store(invocation)?;
        let mut state = store.load()?;
        reject_attached_mutation(&state, "message dismiss")?;

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
        reject_attached_mutation(&state, "message restore")?;

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
        reject_attached_mutation(&state, "message apply")?;

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
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Browse, "entry show")?;
        let view = effective.view(ViewSpec::Latest)?;
        let effective_state = effective.state();
        let locator = EntryLocator::parse(&args.locator)?;
        let resolved = resolve_entry(
            &effective_state.lifecore,
            &effective_state.cli,
            &locator,
            EntryExpectation::Any,
            Some(view.end_exclusive()),
        )?;
        let item = effective.entry_record(&view, &resolved)?;

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
        let action = args.selection_action().map_err(AppError::usage)?;
        let mut prepared_type_override = match action.as_ref() {
        Some(SelectionAction::Type {
            command: SelectionTypeCommand::Override(override_args),
        }) => Some((
            TypePatch::parse(override_args.new_name.clone(), &override_args.patch)?,
            parse_expected_head(&override_args.transaction)?,
        )),
        _ => None,
        };
        if let Some(SelectionAction::AddChild(child)) = action.as_ref() {
            parse_field_assignments(&child.fields)?;
            parse_expected_head(&child.transaction)?;
        }

        let store = state_store(invocation)?;
        let mut state = store.load()?;
        let mutation_operation = match action.as_ref() {
        Some(SelectionAction::Alias { .. }) => Some("query nodes --select -- alias set"),
        Some(SelectionAction::Type { .. }) => Some("query nodes --select -- type override"),
        Some(SelectionAction::AddChild(_)) => Some("query nodes --select -- add-child"),
        None | Some(SelectionAction::Entry { .. }) => None,
        };
        if let Some(operation) = mutation_operation {
            reject_attached_mutation(&state, operation)?;
        }
        let registry = AnalyzerRegistry::built_in();
        let effective = effective_graph(
            &state,
            &registry,
            EffectiveCapability::Browse,
            "query nodes",
        )?;
        let effective_state = effective.state();
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let view = effective.view(view_spec)?;
        let query = parse_query(&view, args.combinator, &args.predicates)?;

        if let Some(locator_text) = args.explain.as_deref() {
            let locator = EntryLocator::parse(locator_text)?;
            let resolved = resolve_entry(
                &effective_state.lifecore,
                &effective_state.cli,
                &locator,
                EntryExpectation::Node,
                Some(view.end_exclusive()),
            )?;
            let candidate = view
                .context()
                .look_at(resolved.entry.entry_context().absolute());
            let explanation = explain_query(&effective_state.lifecore, &query, candidate)?;
            let candidate = effective.entry_record(&view, &resolved)?;
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

        let results = execute_query(&effective_state.lifecore, &query);
        if let Some(locator_text) = args.select.as_deref() {
            let locator = EntryLocator::parse(locator_text)?;
            let resolved = resolve_entry(
                &effective_state.lifecore,
                &effective_state.cli,
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

            match action {
            None
            | Some(SelectionAction::Entry {
                command: SelectionEntryCommand::Inspect,
            }) => {
                let item = effective.entry_record(&view, &resolved)?;

                return Ok(CommandOutcome::Entries {
                    format,
                    view: view.summary(),
                    items: vec![item],
                    detail: true,
                });
            }
            Some(SelectionAction::Alias {
                command: SelectionAliasCommand::Set { name },
            }) => {
                return bind_alias(&store, &mut state, name, &resolved, format);
            }
            Some(SelectionAction::AddChild(child)) => {
                let body = parse_field_assignments(&child.fields)?;
                let expected_head = parse_expected_head(&child.transaction)?;
                let request = AddRequest {
                    type_name: child.r#type,
                    body,
                    parent: Some(EntryIndex(resolved.index())),
                };

                return execute_transaction(
                    &store,
                    &state,
                    request.into_plan(),
                    &child.transaction,
                    expected_head,
                    format,
                );
            }
            Some(SelectionAction::Type {
                command: SelectionTypeCommand::Override(override_args),
            }) => {
                let (patch, expected_head) = prepared_type_override
                    .take()
                    .expect("selected type override was prepared before state access");
                let source = resolve_selected_type(&effective_state.lifecore, &view, resolved)?;
                let parent = EntryIndex(source.entry.index());
                let declaration = patch.apply(&source.declaration)?;

                return execute_transaction(
                    &store,
                    &state,
                    plan_type_append(declaration, Some(parent)),
                    &override_args.transaction,
                    expected_head,
                    format,
                );
            }
            }
        }

        let items = results
            .into_iter()
            .map(|node| {
                resolve_index(
                    &effective_state.lifecore,
                    &effective_state.cli,
                    node.e.entry().0,
                    EntryExpectation::Node,
                    Some(view.end_exclusive()),
                )
                .and_then(|resolved| effective.entry_record(&view, &resolved))
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
        let registry = AnalyzerRegistry::built_in();
        let effective =
            effective_graph(&state, &registry, EffectiveCapability::Browse, "graphviz")?;
        let effective_state = effective.state();
        let view_spec = args
            .at
            .as_deref()
            .map(EntryLocator::parse)
            .transpose()?
            .map_or(ViewSpec::Latest, ViewSpec::Through);
        let view = effective.view(view_spec)?;
        let options = GraphvizOptions {
            full: args.full,
            fields: args.fields,
        };
        let mut aliases = visible_aliases(&effective_state.lifecore, &effective_state.cli, &view)?;
        for names in aliases.values_mut() {
            for name in names {
                if effective.is_temporary_alias(name) {
                    name.push_str(" (temporary)");
                }
            }
        }
        let document = generate(
            GraphvizInput {
                graph: &effective_state.lifecore,
                state: &effective_state.cli,
                view: &view,
                aliases: &aliases,
                canonical_end: effective.canonical_end(),
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
        log::trace!("operation '{operation}' is unavailable in stage 9");

        Err(AppError::unavailable(operation, format))
    }
    }
}

fn parse_expected_head(args: &TransactionArgs) -> Result<Option<ExpectedHead>, AppError> {
    args.expect_head
        .as_deref()
        .map(str::parse::<ExpectedHead>)
        .transpose()
}

fn bind_alias(
    store: &StateStore,
    state: &mut PersistedState,
    name: AliasName,
    target: &ResolvedEntry,
    format: crate::cli::OutputFormat,
) -> Result<CommandOutcome, AppError> {
    let locator = target.full_locator();
    let display_name = name.to_string();
    state.cli.aliases.bind(name, target.durable_locator());
    store.save(state)?;

    Ok(CommandOutcome::AliasChanged {
        format,
        alias: AliasChangeRecord {
            action: "set".to_string(),
            name: display_name,
            locator: Some(locator),
        },
    })
}

fn execute_transaction(
    store: &StateStore,
    state: &PersistedState,
    change: crate::transaction::PlannedChange,
    args: &TransactionArgs,
    expected_head: Option<ExpectedHead>,
    format: crate::cli::OutputFormat,
) -> Result<CommandOutcome, AppError> {
    let registry = AnalyzerRegistry::built_in();
    let request = TransactionRequest {
        change,
        commit: args.commit,
        expected_head,
        dismiss_after_commit: None,
    };
    let result = TransactionService::new(store, &registry).execute(state, request)?;

    Ok(CommandOutcome::Transaction { format, result })
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

fn effective_graph(
    state: &PersistedState,
    registry: &AnalyzerRegistry,
    capability: EffectiveCapability,
    operation: &str,
) -> Result<EffectiveGraph, AppError> {
    prepare_effective(state, registry).require(capability, operation)
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
