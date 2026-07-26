pub mod analysis;
pub mod app;
pub mod cli;
pub mod commands;
pub mod effective;
pub mod entry;
pub mod error;
pub mod graphviz;
mod interactive_analyzer;
pub mod locator;
pub mod messages;
pub mod output;
pub mod payload_input;
pub mod query;
pub mod reference;
pub mod sidecar;
pub mod state;
pub mod transaction;
pub mod type_edit;
pub mod view;

use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory, Parser, error::ErrorKind};

use crate::{
    app::CommandOutcome,
    cli::{Cli, Command},
    error::AppError,
    output::{render_error, render_outcome},
};

#[cfg(test)]
mod test;

pub struct Io<'a> {
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    pub stdout_is_terminal: bool,
}

impl<'a> Io<'a> {
    pub fn new(
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
        stdout_is_terminal: bool,
    ) -> Self {
        Self {
            stdout,
            stderr,
            stdout_is_terminal,
        }
    }
}

#[derive(Clone, Debug)]
pub struct InvocationContext {
    pub current_directory: PathBuf,
    pub state_path_environment: Option<OsString>,
}

impl InvocationContext {
    pub fn new(
        current_directory: impl Into<PathBuf>,
        state_path_environment: Option<OsString>,
    ) -> Self {
        Self {
            current_directory: current_directory.into(),
            state_path_environment,
        }
    }

    pub fn from_process() -> Result<Self, AppError> {
        Ok(Self {
            current_directory: std::env::current_dir()?,
            state_path_environment: std::env::var_os(state::STATE_PATH_ENVIRONMENT),
        })
    }

    pub fn current_directory(&self) -> &Path {
        &self.current_directory
    }
}

enum ParsedInvocation {
    Immediate(CommandOutcome),
    Command(Command),
}

pub fn run_from<I, T>(args: I, io: &Io<'_>) -> Result<CommandOutcome, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    //A Complete clap parsing and validation before process-context lookup so
    //A help, version, and rejected grammar cannot reach filesystem-backed state.
    let parsed = parse_invocation(args)?;

    match parsed {
    ParsedInvocation::Immediate(outcome) => Ok(outcome),
    ParsedInvocation::Command(command) => {
        let invocation = InvocationContext::from_process()?;

        dispatch_command(command, io, &invocation)
    }
    }
}

pub fn run_from_in<I, T>(
    args: I,
    io: &Io<'_>,
    invocation: &InvocationContext,
) -> Result<CommandOutcome, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match parse_invocation(args)? {
    ParsedInvocation::Immediate(outcome) => Ok(outcome),
    ParsedInvocation::Command(command) => dispatch_command(command, io, invocation),
    }
}

fn parse_invocation<I, T>(args: I) -> Result<ParsedInvocation, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
    Ok(cli) => cli,
    Err(error)
        if matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        ) =>
    {
        return Ok(ParsedInvocation::Immediate(CommandOutcome::text(
            error.to_string(),
        )));
    }
    Err(error) => return Err(AppError::usage(error)),
    };

    cli.validate().map_err(AppError::usage)?;

    let command = cli.command.ok_or_else(|| {
        AppError::usage(Cli::command().error(ErrorKind::MissingSubcommand, "a command is required"))
    })?;

    Ok(ParsedInvocation::Command(command))
}

fn dispatch_command(
    command: Command,
    io: &Io<'_>,
    invocation: &InvocationContext,
) -> Result<CommandOutcome, AppError> {
    let format = command.output_format();

    log::trace!("dispatching command (stdout_tty={})", io.stdout_is_terminal);

    commands::dispatch(command, invocation).map_err(|error| error.with_format(format))
}

pub fn run_cli<I, T>(args: I, io: &mut Io<'_>) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match run_from(args, io) {
    Ok(outcome) => match render_outcome(&outcome, io.stdout, io.stdout_is_terminal) {
    Ok(()) => ExitCode::SUCCESS,
    Err(_) => ExitCode::FAILURE,
    },
    Err(error) => {
        let exit_code = error.exit_code();

        if render_error(&error, io.stderr).is_err() {
            return ExitCode::FAILURE;
        }

        ExitCode::from(exit_code)
    }
    }
}
