use clap::{Args, Parser, Subcommand, ValueEnum};
use lifecore::analysis::{AnalyzerFactory, Config};
use lifecore_app::{
    AppError, Service,
    proposal::{self, Format, Proposal},
    rpc,
    service::{AddAnalyzerRequest, NoParams, ProposalRequest},
    storage::StatePath,
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_typecheck::TypeCheckFactory;
use serde::Serialize;
use std::{
    fs,
    io::{self, BufRead, Write},
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
        config: String,
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
    evaluation_time: Option<jiff::Timestamp>,
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

//A Dedicated commands print the same result or error object an RPC call would
//A carry, as JSON on stdout; a short diagnostic also goes to stderr on failure.
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

//A Reads at most one line: a newline or EOF ends it, and nothing after it is
//A read or executed. Exactly one compact response line is written.
fn run_rpc(
    input: &mut impl BufRead,
    output: &mut impl Write,
    service: impl FnOnce() -> Result<Service, AppError>,
) -> io::Result<bool> {
    let mut line = String::new();
    let response = match input.read_line(&mut line) {
    Ok(_) => match rpc::parse_line(line.trim_end_matches(['\n', '\r'])) {
    Ok(request) => match service() {
    Ok(service) => request.respond(&service),
    Err(error) => request.fail(&error),
    },
    Err(response) => response,
    },
    Err(error) => rpc::parse_error(format!("cannot read the request line: {error}")),
    };
    serde_json::to_writer(&mut *output, &response)?;
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
                config: "{}".into(),
            }]
        };
        emit(service().and_then(|service| service.init(&suite)))
    }
    Command::Rpc => match run_rpc(&mut io::stdin().lock(), &mut io::stdout().lock(), service) {
    Ok(true) => ExitCode::SUCCESS,
    Ok(false) => ExitCode::FAILURE,
    Err(error) => {
        eprintln!("lifecore: cannot write the response: {error}");
        ExitCode::FAILURE
    }
    },
    Command::Check { file, evaluation } => {
        let checked =
            proposal_request(&file, evaluation).and_then(|request| service()?.check(request));
        //A A rejected preview is a result, not an error, but the command still
        //A reports it through its exit status.
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
        config,
        evaluation,
    } => emit(service().and_then(|service| {
        service.add_analyzer(AddAnalyzerRequest {
            name,
            configuration: config,
            evaluation_time: evaluation.evaluation_time,
            now: evaluation.now,
        })
    })),
    }
}

#[cfg(test)]
mod test;
