use std::io::{self, IsTerminal};

use env_logger::{Env, Target};
use lifecore_cli::{Io, run_cli};

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(Env::default().default_filter_or("trace"))
        .target(Target::Stderr)
        .init();

    let stdin_is_terminal = io::stdin().is_terminal();
    let stdout_is_terminal = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut io = Io::new(
        &mut stdout,
        &mut stderr,
        stdin_is_terminal,
        stdout_is_terminal,
    );

    run_cli(std::env::args_os(), &mut io)
}
