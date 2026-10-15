use clap::{Args, Parser, Subcommand, ValueEnum};
use lifecore::{
    Timestamp,
    analysis::{AnalyzerFactory, Config},
};
use lifecore_app::{
    AppError, Service,
    proposal::{self, Format, Proposal},
    rpc,
    service::{AddAnalyzerRequest, NoParams, ProposalRequest},
    storage::StatePath,
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_server::{Options, Ui};
use lifecore_typecheck::TypeCheckFactory;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(name = "lifecore", version, about = "LifeCore typed graph database")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Create the selected database without replacing an existing one")]
    Init {
        #[arg(long, help = "Do not configure the Type_Check analyzer")]
        no_typecheck: bool,
    },
    #[command(about = "Serve the browser client and HTTP RPC for the selected database")]
    Serve {
        #[arg(
            long,
            default_value_t = 0,
            help = "Loopback port to listen on; 0 lets the system choose a free one, and 80 is refused"
        )]
        port: u16,
        #[arg(
            long,
            value_name = "DIR",
            help = "Serve index.html and app.js from DIR instead of the embedded client"
        )]
        ui_dir: Option<PathBuf>,
    },
    #[command(about = "Execute one JSON-RPC request line from stdin and print one response line")]
    Rpc,
    #[command(about = "Check a JSON or TOML proposal without storing it")]
    Check {
        file: PathBuf,
        #[command(flatten)]
        evaluation: Evaluation,
    },
    #[command(about = "Check a JSON or TOML proposal anew and store it if accepted")]
    Apply {
        file: PathBuf,
        #[command(flatten)]
        evaluation: Evaluation,
    },
    #[command(about = "Mint a plan ID for the current database and base")]
    PlanId,
    #[command(about = "Convert a proposal file to JSON or TOML on stdout")]
    Export {
        file: PathBuf,
        #[arg(long, value_enum)]
        format: FormatArg,
    },
    #[command(about = "Append a registered analyzer to the ordered analyzer suite")]
    AddAnalyzer {
        name: String,
        #[arg(default_value = "{}")]
        configuration: String,
        #[command(flatten)]
        evaluation: Evaluation,
    },
}

#[derive(Args)]
struct Evaluation {
    #[arg(
        long,
        value_name = "TIMESTAMP",
        conflicts_with = "now",
        help = "Evaluate analyzers at this instant"
    )]
    evaluation_time: Option<Timestamp>,
    #[arg(long, help = "Capture the current instant once and evaluate at it")]
    now: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Json,
    Toml,
}

impl From<FormatArg> for Format {
    fn from(format: FormatArg) -> Self {
        match format {
        FormatArg::Json => Format::Json,
        FormatArg::Toml => Format::Toml,
        }
    }
}

fn factories() -> Vec<Box<dyn AnalyzerFactory>> {
    vec![Box::new(TypeCheckFactory), Box::new(DagCheckFactory)]
}

fn service() -> Result<Service, AppError> {
    Ok(Service::new(StatePath::resolve()?, Box::new(factories)))
}

// The database is selected once, at startup, and must already be readable;
// no remote method selects a path or initializes one. The URL with its token
// is the only line on stdout, so a launcher can read it.
fn serve(port: u16, ui_dir: Option<PathBuf>) -> ExitCode {
    let state = match StatePath::resolve() {
    Ok(state) => state,
    Err(error) => return emit::<()>(Err(error)),
    };
    let service = Service::new(state.clone(), Box::new(factories));
    if let Err(error) = service.describe(NoParams {}) {
        return emit::<()>(Err(error));
    }
    let ui = match ui_dir {
    Some(directory) if !directory.is_dir() => {
        eprintln!(
            "lifecore: --ui-dir {} is not a directory",
            directory.display()
        );
        return ExitCode::FAILURE;
    }
    Some(directory) => Ui::Directory(directory),
    None => Ui::Embedded,
    };
    let served = lifecore_server::serve(service, Options { port, ui }, |url| {
        println!("{url}");
        io::stdout().flush().ok();
        let origin = url.split('#').next().unwrap_or(url);
        eprintln!(
            "lifecore: serving {} at {origin}; open the URL above, and press Ctrl-C to stop",
            state.path().display()
        );
    });
    match served {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
        eprintln!("lifecore: cannot serve: {error}");
        ExitCode::FAILURE
    }
    }
}

fn read_proposal(file: &Path) -> Result<Proposal, AppError> {
    let format = Format::from_path(file).ok_or_else(|| AppError::InvalidParams {
        message: format!(
            "cannot infer the format of {}; use a .json or .toml extension",
            file.display()
        ),
    })?;
    let text = fs::read_to_string(file).map_err(|error| AppError::InvalidParams {
        message: format!("cannot read {}: {error}", file.display()),
    })?;
    proposal::decode(&text, format).map_err(AppError::MalformedProposal)
}

fn proposal_request(file: &Path, evaluation: Evaluation) -> Result<ProposalRequest, AppError> {
    Ok(ProposalRequest {
        proposal: read_proposal(file)?,
        evaluation_time: evaluation.evaluation_time,
        now: evaluation.now,
    })
}

// Dedicated commands print the same result or error object an RPC call would
// carry, as JSON on stdout; a short diagnostic also goes to stderr on failure.
fn emit<T: Serialize>(result: Result<T, AppError>) -> ExitCode {
    let (text, code) = match result {
    Ok(value) => (serde_json::to_string_pretty(&value), ExitCode::SUCCESS),
    Err(error) => {
        eprintln!("lifecore: {error}");
        (
            serde_json::to_string_pretty(&error.to_object()),
            ExitCode::FAILURE,
        )
    }
    };
    match text {
    Ok(text) => {
        println!("{text}");
        code
    }
    Err(error) => {
        eprintln!("lifecore: cannot serialize the result: {error}");
        ExitCode::FAILURE
    }
    }
}

// Consumes bytes through the first newline, or to EOF, and returns them
// without the newline. Callers pass an unbuffered reader, so every byte after
// the newline stays unread for whoever holds the input next.
fn read_line(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut line = vec![];
    let mut byte = [0];
    loop {
        match input.read(&mut byte) {
        Ok(0) => return Ok(line),
        Ok(_) if byte[0] == b'\n' => return Ok(line),
        Ok(_) => line.push(byte[0]),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
        Err(error) => return Err(error),
        }
    }
}

// The standard stdin handle buffers up to 8 KiB, which would consume input
// after the request line. A duplicate of the descriptor reads the same open
// file unbuffered.
fn raw_stdin() -> io::Result<File> {
    let handle = std::os::fd::AsFd::as_fd(&io::stdin()).try_clone_to_owned()?;
    Ok(File::from(handle))
}

// Reads at most one line: a newline or EOF ends it, and nothing after it is
// read or executed. Exactly one compact response line is written.
fn run_rpc(
    input: &mut impl Read,
    output: &mut impl Write,
    service: impl FnOnce() -> Result<Service, AppError>,
) -> io::Result<bool> {
    let line = read_line(input).and_then(|line| {
        String::from_utf8(line).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    });
    let response = match line {
    Ok(line) => match rpc::parse_request(line.strip_suffix('\r').unwrap_or(&line)) {
    Ok(request) => match service() {
    Ok(service) => request.respond(&service),
    Err(error) => request.fail(&error),
    },
    Err(response) => response,
    },
    Err(error) => rpc::parse_error(format!("cannot read the request line: {error}")),
    };
    write_response(output, &response)
}

fn write_response(output: &mut impl Write, response: &rpc::Response) -> io::Result<bool> {
    serde_json::to_writer(&mut *output, response)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(response.is_success())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
    Command::Init { no_typecheck } => {
        let suite = if no_typecheck {
            vec![]
        } else {
            vec![Config {
                name: TypeCheckFactory::name().into(),
                configuration: "{}".into(),
            }]
        };
        emit(service().and_then(|service| service.init(&suite)))
    }
    Command::Serve { port, ui_dir } => serve(port, ui_dir),
    Command::Rpc => {
        let mut output = io::stdout().lock();
        let responded = match raw_stdin() {
        Ok(mut input) => run_rpc(&mut input, &mut output, service),
        Err(error) => write_response(
            &mut output,
            &rpc::parse_error(format!("cannot read the request line: {error}")),
        ),
        };
        match responded {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("lifecore: cannot write the response: {error}");
            ExitCode::FAILURE
        }
        }
    }
    Command::Check { file, evaluation } => {
        let checked =
            proposal_request(&file, evaluation).and_then(|request| service()?.check(request));
        // A rejected preview is a result, not an error, but the command still
        // reports it through its exit status.
        let accepted = checked.as_ref().map_or(true, |result| result.accepted);
        let code = emit(checked);
        if accepted {
            code
        } else {
            eprintln!("lifecore: the proposal is not accepted; analysis emitted inconsistencies");
            ExitCode::FAILURE
        }
    }
    Command::Apply { file, evaluation } => {
        emit(proposal_request(&file, evaluation).and_then(|request| service()?.apply(request)))
    }
    Command::PlanId => emit(service().and_then(|service| service.mint_plan_id(NoParams {}))),
    Command::Export { file, format } => {
        let exported = read_proposal(&file).and_then(|proposal| {
            proposal::encode(&proposal, format.into())
                .map_err(|message| AppError::Internal { message })
        });
        match exported {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => emit::<()>(Err(error)),
        }
    }
    Command::AddAnalyzer {
        name,
        configuration,
        evaluation,
    } => emit(service().and_then(|service| {
        service.add_analyzer(AddAnalyzerRequest {
            name,
            configuration,
            evaluation_time: evaluation.evaluation_time,
            now: evaluation.now,
        })
    })),
    }
}

#[cfg(test)]
mod test;
