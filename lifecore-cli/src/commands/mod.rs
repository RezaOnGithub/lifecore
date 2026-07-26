use crate::{
    InvocationContext,
    analysis::{AnalyzerRegistry, initial_graph, run_analysis},
    app::{CommandOutcome, ViewSummary},
    cli::{Command, MessageCommand},
    error::AppError,
    messages::{MessageUniverse, Severity},
    state::{PersistedState, StatePath, StateStore},
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
                end: universe.entry_count(),
            },
            messages: universe.visible(),
        })
    }
    Command::Check(args)
        if !args.include_dismissed
            && !args.only_dismissed
            && !args.include_resolved
            && !args.dismissal_ids =>
    {
        let store = state_store(invocation)?;
        let state = store.load()?;
        reject_attached_sidecar(&state, "check")?;

        let report = run_analysis(&state.lifecore, &AnalyzerRegistry::built_in())?;
        let universe =
            MessageUniverse::from_analysis(&report, &state.lifecore, &state.cli.dismissals)?;

        Ok(CommandOutcome::Messages {
            format,
            view: ViewSummary {
                end: universe.entry_count(),
            },
            items: universe.visible(),
            detail: false,
        })
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
                end: universe.entry_count(),
            },
            items: vec![message],
            detail: true,
        })
    }
    _ => {
        log::trace!("operation '{operation}' is unavailable in stage 2");

        Err(AppError::unavailable(operation, format))
    }
    }
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
