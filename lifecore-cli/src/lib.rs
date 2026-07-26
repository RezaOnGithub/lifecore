pub mod analysis;
pub mod app;
pub mod cli;
pub mod commands;
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
    cli::Cli,
    error::AppError,
    output::{render_error, render_outcome},
};

#[cfg(test)]
mod test;

pub struct Io<'a> {
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    pub stdin_is_terminal: bool,
    pub stdout_is_terminal: bool,
}

impl<'a> Io<'a> {
    pub fn new(
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
        stdin_is_terminal: bool,
        stdout_is_terminal: bool,
    ) -> Self {
        Self {
            stdout,
            stderr,
            stdin_is_terminal,
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

pub fn run_from<I, T>(args: I, io: &Io<'_>) -> Result<CommandOutcome, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let invocation = InvocationContext::from_process()?;

    run_from_in(args, io, &invocation)
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
    let cli = match Cli::try_parse_from(args) {
    Ok(cli) => cli,
    Err(error)
        if matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        ) =>
    {
        return Ok(CommandOutcome::text(error.to_string()));
    }
    Err(error) => return Err(AppError::usage(error)),
    };

    cli.validate().map_err(AppError::usage)?;

    let command = cli.command.ok_or_else(|| {
        AppError::usage(Cli::command().error(ErrorKind::MissingSubcommand, "a command is required"))
    })?;
    let format = command.output_format();

    log::trace!(
        "dispatching command (stdin_tty={}, stdout_tty={})",
        io.stdin_is_terminal,
        io.stdout_is_terminal
    );

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
