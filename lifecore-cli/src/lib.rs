pub mod diagnostics;
pub mod graphviz;
pub mod interactive_analyzer;
pub mod sidecar;
pub mod state;
mod util;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use diagnostics::{OutputFormat, print_diagnostics, print_rows, print_validation_failure};
use lifecore::{
    EventExt, LifeCore, LifeCoreError, LifeCoreList, Body, NodeExt, Payload,
    PointedEventContext,
    analysis::{AnalysisRunner, Message, Output, SuggestedAction},
};
use lifecore_typecheck::TypeCheckFactory;
use regex::Regex;
use serde_json::{Value, json};

use crate::{
    diagnostics::{message_hash, visible_messages},
    interactive_analyzer::InteractiveFactory,
    sidecar::Sidecar,
    state::{CliState, StateFile, absolute_existing_path, state_path},
    util::{bounded_context, latest_context, pointed_index, same_event},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "A CLI for the LifeCore typed, versioned graph database"
)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t, global = true)]
    pub output: OutputFormat,
    #[arg(long, global = true)]
    pub no_bark: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create an empty state file with the default analysis suite.
    Init,
    /// Run basic validation and the configured analysis suite.
    Check,
    /// Provisionally add one node.
    Add(AddArgs),
    /// Provisionally connect two aliased nodes.
    Connect(ConnectArgs),
    /// Manage configured analyzers.
    Analyzer(AnalyzerArgs),
    /// Manage node aliases.
    Alias(AliasArgs),
    /// Manage ignored or resolved diagnostics.
    Bark(BarkArgs),
    /// Inspect or apply analyzer suggestions.
    Suggestion(SuggestionArgs),
    /// Query graph events.
    Query(QueryArgs),
    /// Manage a TOML sidecar.
    Sidecar(SidecarArgs),
    /// Generate a GraphViz DOT definition or SVG.
    Graphviz(GraphvizArgs),
}

#[derive(Args, Debug)]
pub struct AddArgs {
    pub ty: String,
    #[arg(long)]
    pub parent: Option<String>,
    #[arg(long)]
    pub alias: Option<String>,
    #[arg(long, value_name = "KEY=VALUE")]
    pub text: Vec<String>,
    #[arg(long, value_name = "KEY=VALUE")]
    pub number: Vec<String>,
    #[arg(long)]
    pub commit: bool,
}

#[derive(Args, Debug)]
pub struct ConnectArgs {
    pub from_alias: String,
    pub ty: String,
    pub to_alias: String,
    #[arg(long)]
    pub commit: bool,
}

#[derive(Args, Debug)]
pub struct AnalyzerArgs {
    #[command(subcommand)]
    pub command: AnalyzerCommand,
}

#[derive(Subcommand, Debug)]
pub enum AnalyzerCommand {
    Add {
        name: String,
        config: String,
        #[arg(long)]
        commit: bool,
    },
}

#[derive(Args, Debug)]
pub struct AliasArgs {
    #[command(subcommand)]
    pub command: AliasCommand,
}

#[derive(Subcommand, Debug)]
pub enum AliasCommand {
    List,
    Add {
        name: String,
        #[command(flatten)]
        query: QueryPredicates,
        #[arg(long)]
        select: Option<usize>,
    },
    Update {
        name: String,
        #[arg(long)]
        select: Option<usize>,
    },
    Remove {
        name: String,
    },
}

#[derive(Args, Debug)]
pub struct BarkArgs {
    #[command(subcommand)]
    pub command: BarkCommand,
}

#[derive(Subcommand, Debug)]
pub enum BarkCommand {
    List,
    Message {
        #[arg(long)]
        select: Option<usize>,
    },
    Resolve {
        #[arg(long)]
        select: Option<usize>,
    },
}

#[derive(Args, Debug)]
pub struct SuggestionArgs {
    #[command(subcommand)]
    pub command: SuggestionCommand,
}

#[derive(Subcommand, Debug)]
pub enum SuggestionCommand {
    Apply {
        #[arg(long)]
        select: Option<usize>,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        commit: bool,
    },
}

#[derive(Args, Clone, Debug, Default)]
pub struct QueryPredicates {
    #[arg(long, value_name = "ALIAS")]
    pub fresh_by: Vec<String>,
    #[arg(long = "type", value_name = "TYPE")]
    pub type_name: Vec<String>,
    #[arg(long, value_name = "REGEX")]
    pub match_text: Vec<String>,
}

#[derive(Args, Debug)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QueryCommand,
}

#[derive(Subcommand, Debug)]
pub enum QueryCommand {
    Filter(QueryPredicates),
    Pick(QueryPredicates),
}

#[derive(Args, Debug)]
pub struct SidecarArgs {
    #[command(subcommand)]
    pub command: SidecarCommand,
}

#[derive(Subcommand, Debug)]
pub enum SidecarCommand {
    Attach { filename: PathBuf },
    Detach,
    Commit,
    Dump,
}

#[derive(Args, Debug)]
pub struct GraphvizArgs {
    pub target: PathBuf,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub fields: bool,
    #[arg(long)]
    pub dot: bool,
}

struct PreparedState {
    state: StateFile,
    aliases: BTreeMap<String, PointedEventContext>,
    sidecar_aliases: BTreeMap<String, PointedEventContext>,
    added_events: Vec<PointedEventContext>,
    last_analysis: Option<Output>,
}

impl Cli {
    pub fn run() -> Result<()> {
        Self::parse().execute()
    }

    pub fn execute(self) -> Result<()> {
        let path = state_path()?;
        self.execute_at(&path)
    }

    pub fn execute_at(mut self, path: &Path) -> Result<()> {
        log::trace!(
            target: "lifecore_cli",
            "executing command {:?} using state file {}",
            self.command,
            path.display()
        );
        if matches!(self.command, Command::Init) {
            return self.initialize(path);
        }

        let loaded = StateFile::load(path)?;
        let command = std::mem::replace(&mut self.command, Command::Check);
        match command {
        Command::Init => unreachable!(),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach { filename },
        }) => self.attach_sidecar(path, loaded, &filename),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Detach,
        }) => self.detach_sidecar(path, loaded),
        command => self.execute_loaded(path, loaded, command),
        }
    }

    fn initialize(&self, path: &Path) -> Result<()> {
        let mut state = StateFile::empty();
        state
            .lifecore
            .add_analyzer(InteractiveFactory::<LifeCoreList>::name(), "");
        state
            .lifecore
            .add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "");
        if let Err(failure) = state.lifecore.basic_validation() {
            print_validation_failure(&failure, self.output)?;
            bail!("initial database failed basic validation");
        }
        let output = run_analysis(&state.lifecore)?;
        let visible = visible_messages(output.0.clone(), &state.cli.barklist, self.no_bark);
        print_diagnostics(&visible, self.output)?;
        if output.has_error() {
            bail!("initial database failed analysis");
        }
        state.lifecore.apply()?;
        state.create(path)?;
        eprintln!(
            "Initialized {} with analyzers {:?} and {:?}.",
            path.display(),
            InteractiveFactory::<LifeCoreList>::name(),
            TypeCheckFactory::<LifeCoreList>::name()
        );
        Ok(())
    }

    fn execute_loaded(&self, path: &Path, loaded: StateFile, command: Command) -> Result<()> {
        let attached = loaded.cli.sidecar.is_some();
        if attached && command_is_forbidden_with_sidecar(&command) {
            bail!("this operation is unavailable while a sidecar is attached");
        }

        let mut prepared = if attached {
            prepare_sidecar(loaded, self.output, self.no_bark)?
        } else {
            PreparedState {
                aliases: loaded.cli.aliases.clone(),
                state: loaded,
                sidecar_aliases: BTreeMap::new(),
                added_events: Vec::new(),
                last_analysis: None,
            }
        };

        match command {
        Command::Check => self.check(&prepared),
        Command::Add(arguments) => self.add(path, prepared.state, arguments),
        Command::Connect(arguments) => self.connect(path, prepared.state, arguments),
        Command::Analyzer(arguments) => self.analyzer(path, prepared.state, arguments),
        Command::Alias(arguments) => self.alias(path, &mut prepared, arguments),
        Command::Bark(arguments) => self.bark(path, &mut prepared, arguments),
        Command::Suggestion(arguments) => self.suggestion(path, prepared.state, arguments),
        Command::Query(arguments) => self.query(&prepared, arguments),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Commit,
        }) => self.commit_sidecar(path, prepared),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Dump,
        }) => self.dump_sidecar(&prepared),
        Command::Graphviz(arguments) => self.graphviz(&prepared, arguments),
        Command::Init
        | Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach { .. } | SidecarCommand::Detach,
        }) => unreachable!(),
        }
    }

    fn check(&self, prepared: &PreparedState) -> Result<()> {
        if let Err(failure) = prepared.state.lifecore.basic_validation() {
            print_validation_failure(&failure, self.output)?;
            bail!("basic validation failed");
        }
        let output = run_analysis(&prepared.state.lifecore)?;
        let mut visible =
            visible_messages(output.0.clone(), &prepared.state.cli.barklist, self.no_bark);
        if let Some(first_error) = visible
            .iter()
            .position(|message| matches!(message, Message::Inconsistency(_)))
        {
            visible.truncate(first_error + 1);
        }
        print_diagnostics(&visible, self.output)?;
        if output.has_error() {
            bail!("analysis found an inconsistency");
        }
        Ok(())
    }

    fn add(&self, path: &Path, mut state: StateFile, arguments: AddArgs) -> Result<()> {
        if let Some(alias) = &arguments.alias {
            ensure_new_alias(&state.cli, alias)?;
        }
        let parent = arguments
            .parent
            .as_deref()
            .map(|alias| resolve_alias(&state.lifecore, &state.cli.aliases, alias))
            .transpose()?;
        let body = parse_body(&arguments.text, &arguments.number)?;
        let (node, output) = stage_node(
            &mut state.lifecore,
            &arguments.ty,
            body,
            parent,
            &state.cli,
            self.output,
            self.no_bark,
        )?;
        if let Some(alias) = arguments.alias {
            state.cli.aliases.insert(alias, bounded_context(node.e));
        }
        self.print_analysis(&output, &state.cli)?;
        if arguments.commit {
            apply_and_save(&mut state, path)?;
            eprintln!("Committed node {}.", node.e);
        } else {
            eprintln!("Dry run: node {} was not persisted.", node.e);
        }
        Ok(())
    }

    fn connect(&self, path: &Path, mut state: StateFile, arguments: ConnectArgs) -> Result<()> {
        let from = resolve_alias(&state.lifecore, &state.cli.aliases, &arguments.from_alias)?;
        let to = resolve_alias(&state.lifecore, &state.cli.aliases, &arguments.to_alias)?;
        let (edge, output) = stage_edge(
            &mut state.lifecore,
            from,
            &arguments.ty,
            to,
            &state.cli,
            self.output,
            self.no_bark,
        )?;
        self.print_analysis(&output, &state.cli)?;
        if arguments.commit {
            apply_and_save(&mut state, path)?;
            eprintln!("Committed edge {}.", edge.e);
        } else {
            eprintln!("Dry run: edge {} was not persisted.", edge.e);
        }
        Ok(())
    }

    fn analyzer(&self, path: &Path, mut state: StateFile, arguments: AnalyzerArgs) -> Result<()> {
        match arguments.command {
        AnalyzerCommand::Add {
            name,
            config,
            commit,
        } => {
            state.lifecore.add_analyzer(&name, &config);
            if let Err(failure) = state.lifecore.basic_validation() {
                print_validation_failure(&failure, self.output)?;
                bail!("analyzer candidate failed basic validation");
            }
            let output =
                analyzed_or_reported(&state.lifecore, &state.cli, self.output, self.no_bark)?;
            self.print_analysis(&output, &state.cli)?;
            if commit {
                apply_and_save(&mut state, path)?;
                eprintln!("Committed analyzer {name:?} with config {config:?}.");
            } else {
                eprintln!("Dry run: analyzer configuration was not persisted.");
            }
            Ok(())
        }
        }
    }

    fn alias(&self, path: &Path, prepared: &mut PreparedState, arguments: AliasArgs) -> Result<()> {
        match arguments.command {
        AliasCommand::List => {
            print_aliases(&prepared.state.lifecore, &prepared.aliases, self.output)
        }
        AliasCommand::Add {
            name,
            query,
            select,
        } => {
            ensure_new_alias(&prepared.state.cli, &name)?;
            let events = query_events(
                &prepared.state.lifecore,
                &prepared.aliases,
                &query,
                QueryMode::Filter,
            )?
            .into_iter()
            .filter(EventExt::is_node)
            .collect::<Vec<_>>();
            let Some(selection) = select else {
                return print_events(&events, &prepared.aliases, self.output);
            };
            let selected = selected(&events, selection, "node query result")?;
            prepared
                .state
                .cli
                .aliases
                .insert(name.clone(), bounded_context(selected.ctx()));
            prepared.state.save(path)?;
            eprintln!("Alias {name:?} now points to {}.", selected.ctx());
            Ok(())
        }
        AliasCommand::Update { name, select } => {
            let old = *prepared
                .state
                .cli
                .aliases
                .get(&name)
                .ok_or_else(|| anyhow!("alias {name:?} does not exist"))?;
            let latest = latest_context(&prepared.state.lifecore);
            let old_now = latest.look_at(old);
            if prepared.state.lifecore.is_stale(old_now) != Some(true) {
                bail!("alias {name:?} does not point to a stale node");
            }
            let successors = prepared.state.lifecore.fresh_successors(old_now);
            let events = successors
                .into_iter()
                .filter_map(|context| prepared.state.lifecore.get(context))
                .collect::<Vec<_>>();
            let Some(selection) = select else {
                return print_events(&events, &prepared.aliases, self.output);
            };
            let selected = selected(&events, selection, "fresh successor")?;
            prepared
                .state
                .cli
                .aliases
                .insert(name.clone(), bounded_context(selected.ctx()));
            prepared.state.save(path)?;
            eprintln!("Alias {name:?} now points to {}.", selected.ctx());
            Ok(())
        }
        AliasCommand::Remove { name } => {
            if prepared.state.cli.aliases.remove(&name).is_none() {
                bail!("alias {name:?} does not exist");
            }
            prepared.state.save(path)?;
            eprintln!("Removed alias {name:?}.");
            Ok(())
        }
        }
    }

    fn bark(&self, path: &Path, prepared: &mut PreparedState, arguments: BarkArgs) -> Result<()> {
        match arguments.command {
        BarkCommand::List => print_barklist(&prepared.state.cli, &prepared.aliases, self.output),
        BarkCommand::Message { select } => {
            let output = run_analysis(&prepared.state.lifecore)?;
            if output.has_error() {
                let visible =
                    visible_messages(output.0, &prepared.state.cli.barklist, self.no_bark);
                print_diagnostics(&visible, self.output)?;
                bail!("cannot barklist diagnostics while analysis has inconsistencies");
            }
            let candidates = visible_messages(output.0, &prepared.state.cli.barklist, self.no_bark)
                .into_iter()
                .filter(|message| !matches!(message, Message::Inconsistency(_)))
                .collect::<Vec<_>>();
            let Some(selection) = select else {
                return print_diagnostics(&candidates, self.output);
            };
            let message = selected(&candidates, selection, "diagnostic")?;
            let hash = message_hash(message);
            if !prepared.state.cli.barklist.hashes.contains(&hash) {
                prepared.state.cli.barklist.hashes.push(hash.clone());
                prepared.state.cli.barklist.hashes.sort();
            }
            prepared.state.save(path)?;
            eprintln!("Barklisted diagnostic {hash}.");
            Ok(())
        }
        BarkCommand::Resolve { select } => {
            let candidates = prepared.state.cli.aliases.iter().collect::<Vec<_>>();
            let rows = candidates
                .iter()
                .enumerate()
                .map(|(index, (name, context))| {
                    vec![
                        (index + 1).to_string(),
                        (*name).clone(),
                        context.to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            let values = candidates
                    .iter()
                    .enumerate()
                    .map(|(index, (name, context))| {
                        json!({ "index": index + 1, "alias": name, "context": context })
                    })
                    .collect::<Vec<_>>();
            let Some(selection) = select else {
                return print_rows(
                    &["#", "alias", "context"],
                    &rows,
                    &json!(values),
                    self.output,
                );
            };
            let (_, context) = selected(&candidates, selection, "alias")?;
            let bounded = bounded_context(**context);
            if !prepared
                .state
                .cli
                .barklist
                .resolved
                .iter()
                .any(|existing| same_event(*existing, bounded))
            {
                prepared.state.cli.barklist.resolved.push(bounded);
                prepared.state.cli.barklist.resolved.sort();
            }
            prepared.state.save(path)?;
            eprintln!("Marked {} as resolved.", context);
            Ok(())
        }
        }
    }

    fn suggestion(
        &self,
        path: &Path,
        mut state: StateFile,
        arguments: SuggestionArgs,
    ) -> Result<()> {
        match arguments.command {
        SuggestionCommand::Apply {
            select,
            alias,
            commit,
        } => {
            let output =
                analyzed_or_reported(&state.lifecore, &state.cli, self.output, self.no_bark)?;
            let suggestions = visible_messages(output.0, &state.cli.barklist, self.no_bark)
                .into_iter()
                .filter(|message| matches!(message, Message::Suggestion(_)))
                .collect::<Vec<_>>();
            let Some(selection) = select else {
                return print_diagnostics(&suggestions, self.output);
            };
            let selected_message = selected(&suggestions, selection, "suggestion")?.clone();
            let Message::Suggestion(suggestion) = &selected_message else {
                unreachable!()
            };
            let add_node_count = suggestion
                .action
                .iter()
                .filter(|action| matches!(action, SuggestedAction::AddNode { .. }))
                .count();
            if alias.is_some() && add_node_count != 1 {
                bail!("--alias requires a suggestion that adds exactly one node");
            }
            if let Some(alias) = &alias {
                ensure_new_alias(&state.cli, alias)?;
            }

            let mut created_node = None;
            let mut last_output = run_analysis(&state.lifecore)?;
            for action in suggestion.action.clone() {
                match action {
                SuggestedAction::AddNode {
                    ty_name,
                    payload,
                    parent,
                } => {
                    let (node, output) = stage_node(
                        &mut state.lifecore,
                        &ty_name,
                        payload,
                        parent,
                        &state.cli,
                        self.output,
                        self.no_bark,
                    )?;
                    created_node = Some(node.e);
                    last_output = output;
                }
                SuggestedAction::AddEdge { from, ty_name, to } => {
                    let (_, output) = stage_edge(
                        &mut state.lifecore,
                        from,
                        &ty_name,
                        to,
                        &state.cli,
                        self.output,
                        self.no_bark,
                    )?;
                    last_output = output;
                }
                }
            }
            if let (Some(alias), Some(node)) = (alias, created_node) {
                state.cli.aliases.insert(alias, bounded_context(node));
            }
            self.print_analysis(&last_output, &state.cli)?;
            if commit {
                if !self.no_bark {
                    let hash = message_hash(&selected_message);
                    if !state.cli.barklist.hashes.contains(&hash) {
                        state.cli.barklist.hashes.push(hash);
                        state.cli.barklist.hashes.sort();
                    }
                }
                apply_and_save(&mut state, path)?;
                eprintln!("Committed suggestion #{selection}.");
            } else {
                eprintln!("Dry run: suggestion #{selection} was not persisted.");
            }
            Ok(())
        }
        }
    }

    fn query(&self, prepared: &PreparedState, arguments: QueryArgs) -> Result<()> {
        let (mode, predicates) = match arguments.command {
        QueryCommand::Filter(predicates) => (QueryMode::Filter, predicates),
        QueryCommand::Pick(predicates) => (QueryMode::Pick, predicates),
        };
        let events = query_events(
            &prepared.state.lifecore,
            &prepared.aliases,
            &predicates,
            mode,
        )?;
        print_events(&events, &prepared.aliases, self.output)
    }

    fn attach_sidecar(&self, path: &Path, mut state: StateFile, filename: &Path) -> Result<()> {
        if state.cli.sidecar.is_some() {
            bail!("a sidecar is already attached");
        }
        let absolute = absolute_existing_path(filename)?;
        state.cli.sidecar = Some(absolute.clone());
        state.save(path)?;
        eprintln!(
            "Attached sidecar {} without processing it.",
            absolute.display()
        );
        Ok(())
    }

    fn detach_sidecar(&self, path: &Path, mut state: StateFile) -> Result<()> {
        let Some(sidecar) = state.cli.sidecar.take() else {
            bail!("no sidecar is attached");
        };
        state.save(path)?;
        eprintln!("Detached sidecar {} without reading it.", sidecar.display());
        Ok(())
    }

    fn commit_sidecar(&self, path: &Path, mut prepared: PreparedState) -> Result<()> {
        for (name, context) in prepared.sidecar_aliases {
            prepared
                .state
                .cli
                .aliases
                .insert(name, bounded_context(context));
        }
        prepared.state.cli.sidecar = None;
        if let Some(output) = &prepared.last_analysis {
            self.print_analysis(output, &prepared.state.cli)?;
        }
        apply_and_save(&mut prepared.state, path)?;
        eprintln!("Committed the attached sidecar; the sidecar file was not deleted.");
        Ok(())
    }

    fn dump_sidecar(&self, prepared: &PreparedState) -> Result<()> {
        let events = prepared
            .added_events
            .iter()
            .filter_map(|context| prepared.state.lifecore.get(*context))
            .collect::<Vec<_>>();
        print_events(&events, &prepared.aliases, self.output)
    }

    fn graphviz(&self, prepared: &PreparedState, arguments: GraphvizArgs) -> Result<()> {
        let dot = graphviz::render_dot(
            &prepared.state.lifecore,
            &prepared.aliases,
            arguments.full,
            arguments.fields,
        );
        if arguments.dot {
            graphviz::write_dot(&arguments.target, &dot)?;
        } else {
            graphviz::write_svg(&arguments.target, &dot)?;
        }
        eprintln!(
            "Wrote graph visualization to {}.",
            arguments.target.display()
        );
        Ok(())
    }

    fn print_analysis(&self, output: &Output, cli: &CliState) -> Result<()> {
        let visible = visible_messages(output.0.clone(), &cli.barklist, self.no_bark);
        print_diagnostics(&visible, self.output)
    }
}

fn build_runner() -> Result<AnalysisRunner<LifeCoreList>> {
    let mut runner = AnalysisRunner::default();
    runner.register_analyzer_kind(Box::new(InteractiveFactory::<LifeCoreList>::default()))?;
    runner.register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))?;
    Ok(runner)
}

fn run_analysis(lifecore: &LifeCoreList) -> Result<Output> {
    log::trace!(
        "running configured analysis suite with {} graph events",
        lifecore.last().map_or(0, |last| pointed_index(last) + 1)
    );
    build_runner()?.run(lifecore).map_err(Into::into)
}

fn analyzed_or_reported(
    lifecore: &LifeCoreList,
    cli: &CliState,
    format: OutputFormat,
    no_bark: bool,
) -> Result<Output> {
    let output = run_analysis(lifecore)?;
    if output.has_error() {
        let visible = visible_messages(output.0.clone(), &cli.barklist, no_bark);
        print_diagnostics(&visible, format)?;
        bail!("analysis found an inconsistency");
    }
    Ok(output)
}

fn stage_node(
    lifecore: &mut LifeCoreList,
    ty: &str,
    body: Body,
    parent: Option<PointedEventContext>,
    cli: &CliState,
    format: OutputFormat,
    no_bark: bool,
) -> Result<(NodeExt, Output)> {
    log::debug!("staging node of type {ty:?}");
    let node = match lifecore.add_mock(ty, body, parent) {
    Ok(node) => node,
    Err(LifeCoreError::BasicValidationFailure(failure)) => {
        print_validation_failure(&failure, format)?;
        bail!("node failed basic validation");
    }
    Err(error) => return Err(error.into()),
    };
    let output = analyzed_or_reported(lifecore, cli, format, no_bark)?;
    Ok((node, output))
}

fn stage_edge(
    lifecore: &mut LifeCoreList,
    from: PointedEventContext,
    ty: &str,
    to: PointedEventContext,
    cli: &CliState,
    format: OutputFormat,
    no_bark: bool,
) -> Result<(lifecore::EdgeExt, Output)> {
    log::debug!("staging edge of type {ty:?} from {from} to {to}");
    let edge = match lifecore.connect_mock(from, ty, to) {
    Ok(edge) => edge,
    Err(LifeCoreError::BasicValidationFailure(failure)) => {
        print_validation_failure(&failure, format)?;
        bail!("edge failed basic validation");
    }
    Err(error) => return Err(error.into()),
    };
    let output = analyzed_or_reported(lifecore, cli, format, no_bark)?;
    Ok((edge, output))
}

fn apply_and_save(state: &mut StateFile, path: &Path) -> Result<()> {
    state.lifecore.apply()?;
    state.save(path)
}

fn prepare_sidecar(
    mut state: StateFile,
    format: OutputFormat,
    no_bark: bool,
) -> Result<PreparedState> {
    let path = state
        .cli
        .sidecar
        .clone()
        .ok_or_else(|| anyhow!("no sidecar is attached"))?;
    let sidecar = Sidecar::load(&path)?;
    log::debug!(
        "processing {} entries from attached sidecar {}",
        sidecar.entries.len(),
        path.display()
    );
    let base_count = state
        .lifecore
        .last()
        .map_or(0, |last| pointed_index(last) + 1);
    let mut aliases = state.cli.aliases.clone();
    let mut sidecar_aliases = BTreeMap::new();
    let mut last_analysis = None;

    for (entry_index, entry) in sidecar.entries.into_iter().enumerate() {
        log::trace!("processing sidecar entry {}", entry_index + 1);
        if let Some(alias) = &entry.alias
            && aliases.contains_key(alias)
        {
            bail!(
                "sidecar entry {} defines duplicate alias {alias:?}",
                entry_index + 1
            );
        }
        let parent = entry
            .parent
            .as_deref()
            .map(|alias| resolve_alias(&state.lifecore, &aliases, alias))
            .transpose()
            .with_context(|| format!("invalid parent in sidecar entry {}", entry_index + 1))?;
        let body = entry
            .node_body()
            .with_context(|| format!("invalid body in sidecar entry {}", entry_index + 1))?;
        let (node, output) = stage_node(
            &mut state.lifecore,
            &entry.ty,
            body,
            parent,
            &state.cli,
            format,
            no_bark,
        )
        .with_context(|| format!("failed to add sidecar entry {}", entry_index + 1))?;
        last_analysis = Some(output);
        if let Some(alias) = entry.alias {
            aliases.insert(alias.clone(), node.e);
            sidecar_aliases.insert(alias, node.e);
        }

        for connection in entry.connect.connect_to {
            let target = resolve_alias(&state.lifecore, &aliases, &connection.target)
                .with_context(|| {
                    format!(
                        "invalid forward connection in sidecar entry {}",
                        entry_index + 1
                    )
                })?;
            let (_, output) = stage_edge(
                &mut state.lifecore,
                node.e,
                &connection.ty,
                target,
                &state.cli,
                format,
                no_bark,
            )?;
            last_analysis = Some(output);
        }
        for connection in entry.connect.connect_back_from {
            let source = resolve_alias(&state.lifecore, &aliases, &connection.target)
                .with_context(|| {
                    format!(
                        "invalid backward connection in sidecar entry {}",
                        entry_index + 1
                    )
                })?;
            let (_, output) = stage_edge(
                &mut state.lifecore,
                source,
                &connection.ty,
                node.e,
                &state.cli,
                format,
                no_bark,
            )?;
            last_analysis = Some(output);
        }
    }

    let added_events = state
        .lifecore
        .last()
        .map(|last| {
            (base_count..=pointed_index(last))
                .map(PointedEventContext::new)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(PreparedState {
        state,
        aliases,
        sidecar_aliases,
        added_events,
        last_analysis,
    })
}

fn command_is_forbidden_with_sidecar(command: &Command) -> bool {
    matches!(
        command,
        Command::Add(_)
            | Command::Connect(_)
            | Command::Analyzer(_)
            | Command::Suggestion(_)
            | Command::Alias(AliasArgs {
                command: AliasCommand::Add { .. }
                    | AliasCommand::Update { .. }
                    | AliasCommand::Remove { .. },
            })
            | Command::Bark(BarkArgs {
                command: BarkCommand::Message { .. } | BarkCommand::Resolve { .. },
            })
    )
}

fn resolve_alias(
    lifecore: &LifeCoreList,
    aliases: &BTreeMap<String, PointedEventContext>,
    name: &str,
) -> Result<PointedEventContext> {
    let stored = *aliases
        .get(name)
        .ok_or_else(|| anyhow!("alias {name:?} does not exist"))?;
    let latest = latest_context(lifecore);
    let context = latest.look_at(stored);
    if lifecore.node(context).is_none() {
        bail!("alias {name:?} does not identify a node in the current database");
    }
    Ok(context)
}

fn ensure_new_alias(cli: &CliState, name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("alias name cannot be empty");
    }
    if cli.aliases.contains_key(name) {
        bail!("alias {name:?} already exists");
    }
    Ok(())
}

fn parse_body(text: &[String], number: &[String]) -> Result<Body> {
    let mut body = BTreeMap::new();
    for assignment in text {
        let (key, value) = parse_assignment(assignment, "text")?;
        if body
            .insert(key.to_string(), Payload::Text(value.to_string()))
            .is_some()
        {
            bail!("field {key:?} was specified more than once");
        }
    }
    for assignment in number {
        let (key, value) = parse_assignment(assignment, "number")?;
        let value = value
            .parse::<i64>()
            .with_context(|| format!("invalid i64 value in --number {assignment:?}"))?;
        if body
            .insert(key.to_string(), Payload::Number(value))
            .is_some()
        {
            bail!("field {key:?} was specified more than once");
        }
    }
    Ok(Body(body))
}

fn parse_assignment<'a>(assignment: &'a str, option: &str) -> Result<(&'a str, &'a str)> {
    let (key, value) = assignment
        .split_once('=')
        .ok_or_else(|| anyhow!("--{option} value {assignment:?} must use KEY=VALUE syntax"))?;
    if key.is_empty() {
        bail!("--{option} value {assignment:?} has an empty key");
    }
    Ok((key, value))
}

#[derive(Clone, Copy)]
enum QueryMode {
    Filter,
    Pick,
}

enum CompiledPredicate {
    FreshBy(PointedEventContext),
    Type(String),
    MatchText(Regex),
}

fn query_events(
    lifecore: &LifeCoreList,
    aliases: &BTreeMap<String, PointedEventContext>,
    predicates: &QueryPredicates,
    mode: QueryMode,
) -> Result<Vec<EventExt>> {
    let mut compiled = Vec::new();
    for alias in &predicates.fresh_by {
        compiled.push(CompiledPredicate::FreshBy(resolve_alias(
            lifecore, aliases, alias,
        )?));
    }
    compiled.extend(
        predicates
            .type_name
            .iter()
            .cloned()
            .map(CompiledPredicate::Type),
    );
    for pattern in &predicates.match_text {
        compiled.push(CompiledPredicate::MatchText(
            Regex::new(pattern)
                .with_context(|| format!("invalid --match-text regex {pattern:?}"))?,
        ));
    }

    let context = latest_context(lifecore);
    let events =
        lifecore.window(context.items()).filter(|event| {
            let matches =
                |predicate: &CompiledPredicate| match predicate {
                CompiledPredicate::FreshBy(alias) => {
                    let historical = alias.shrink_to_inclusive_left();
                    pointed_index(event.ctx()) < pointed_index(*alias) + 1
                        && lifecore.is_stale(historical.look_at(event.ctx())) == Some(false)
                }
                CompiledPredicate::Type(expected) => match event {
                EventExt::Node(node) => &node.ty_name == expected,
                EventExt::Edge(edge) => &edge.ty_name == expected,
                },
                CompiledPredicate::MatchText(regex) => match event {
                EventExt::Node(node) => node.field.0.values().any(
                    |payload| matches!(payload, Payload::Text(value) if regex.is_match(value)),
                ),
                EventExt::Edge(_) => false,
                },
                };
            match mode {
            QueryMode::Filter => compiled.iter().all(matches),
            QueryMode::Pick => !compiled.is_empty() && compiled.iter().any(matches),
            }
        });
    Ok(events.collect())
}

fn selected<'a, T>(values: &'a [T], selection: usize, kind: &str) -> Result<&'a T> {
    if selection == 0 {
        bail!("--select uses one-based indices; zero is invalid");
    }
    values.get(selection - 1).ok_or_else(|| {
        anyhow!(
            "selected {kind} #{selection}, but the current deterministic list has {} entries",
            values.len()
        )
    })
}

fn print_events(
    events: &[EventExt],
    aliases: &BTreeMap<String, PointedEventContext>,
    format: OutputFormat,
) -> Result<()> {
    let values = events
        .iter()
        .enumerate()
        .map(|(index, event)| event_value(index + 1, event, aliases))
        .collect::<Vec<_>>();
    let rows = values
        .iter()
        .map(|value| {
            vec![
                value["index"].to_string(),
                value["context"].to_string(),
                value["kind"].as_str().unwrap_or_default().to_string(),
                value["type"].as_str().unwrap_or_default().to_string(),
                value["aliases"]
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
                value["details"].to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print_rows(
        &["#", "context", "kind", "type", "aliases", "details"],
        &rows,
        &json!(values),
        format,
    )
}

fn event_value(
    index: usize,
    event: &EventExt,
    aliases: &BTreeMap<String, PointedEventContext>,
) -> Value {
    let matching_aliases = aliases
        .iter()
        .filter(|(_, context)| same_event(**context, event.ctx()))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    match event {
    EventExt::Node(node) => json!({
        "index": index,
        "context": node.e,
        "kind": "node",
        "type": node.ty_name,
        "aliases": matching_aliases,
        "details": { "body": node.field, "parent": node.parent },
    }),
    EventExt::Edge(edge) => json!({
        "index": index,
        "context": edge.e,
        "kind": "edge",
        "type": edge.ty_name,
        "aliases": matching_aliases,
        "details": { "from": edge.from, "to": edge.to },
    }),
    }
}

fn print_aliases(
    lifecore: &LifeCoreList,
    aliases: &BTreeMap<String, PointedEventContext>,
    format: OutputFormat,
) -> Result<()> {
    let latest = latest_context(lifecore);
    let values = aliases
        .iter()
        .map(|(name, stored)| {
            let current = latest.look_at(*stored);
            let node = lifecore.node(current);
            let stale = lifecore.is_stale(current);
            let fresh_successors = if stale == Some(true) {
                lifecore.fresh_successors(current)
            } else {
                Vec::new()
            };
            json!({
                "name": name,
                "context": stored,
                "type": node.as_ref().map(|node| node.ty_name.as_str()),
                "stale": stale,
                "fresh_successors": fresh_successors,
                "body": node.map(|node| node.field),
            })
        })
        .collect::<Vec<_>>();
    let rows = values
        .iter()
        .map(|value| {
            vec![
                value["name"].as_str().unwrap_or_default().to_string(),
                value["context"].to_string(),
                value["type"].as_str().unwrap_or("<missing>").to_string(),
                value["stale"].to_string(),
                value["fresh_successors"].to_string(),
                value["body"].to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print_rows(
        &[
            "alias",
            "context",
            "type",
            "stale",
            "fresh successors",
            "body",
        ],
        &rows,
        &json!(values),
        format,
    )
}

fn print_barklist(
    cli: &CliState,
    aliases: &BTreeMap<String, PointedEventContext>,
    format: OutputFormat,
) -> Result<()> {
    let mut values = cli
        .barklist
        .hashes
        .iter()
        .map(|hash| json!({ "kind": "hash", "value": hash }))
        .collect::<Vec<_>>();
    values.extend(cli.barklist.resolved.iter().map(|context| {
        let names = aliases
            .iter()
            .filter(|(_, candidate)| same_event(**candidate, *context))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        json!({ "kind": "resolved", "value": context, "aliases": names })
    }));
    let rows = values
        .iter()
        .map(|value| {
            vec![
                value["kind"].as_str().unwrap_or_default().to_string(),
                value["value"].to_string(),
                value
                    .get("aliases")
                    .map(Value::to_string)
                    .unwrap_or_default(),
            ]
        })
        .collect::<Vec<_>>();
    print_rows(&["kind", "value", "aliases"], &rows, &json!(values), format)
}

#[cfg(test)]
mod tests {
    use std::{
        fs, process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use clap::Parser;

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lifecore-cli-app-test-{}-{}-{name}.{extension}",
            process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn cli(command: Command) -> Cli {
        Cli {
            output: OutputFormat::Lines,
            no_bark: false,
            command,
        }
    }

    #[test]
    fn parses_add_fields_and_global_options_after_subcommand() {
        let cli = Cli::try_parse_from([
            "lifecore-cli",
            "add",
            "Note",
            "--text",
            "content=Buy milk",
            "--number",
            "priority=2",
            "--commit",
            "--output",
            "json",
        ])
        .unwrap();
        let Command::Add(add) = cli.command else {
            panic!("expected add")
        };
        assert_eq!(add.text, ["content=Buy milk"]);
        assert_eq!(add.number, ["priority=2"]);
        assert!(add.commit);
        assert_eq!(cli.output, OutputFormat::Json);
    }

    #[test]
    fn filter_without_predicates_returns_every_event_and_pick_returns_none() {
        let mut lifecore = LifeCoreList::default();
        lifecore.add("A", Body::default(), None).unwrap();
        lifecore.add("B", Body::default(), None).unwrap();
        let aliases = BTreeMap::new();
        let predicates = QueryPredicates::default();
        assert_eq!(
            query_events(&lifecore, &aliases, &predicates, QueryMode::Filter)
                .unwrap()
                .len(),
            2
        );
        assert!(
            query_events(&lifecore, &aliases, &predicates, QueryMode::Pick)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn query_predicates_cover_type_text_and_historical_freshness() {
        let mut lifecore = LifeCoreList::default();
        let root = lifecore
            .add(
                "Note",
                Body::from(&[("content", Payload::Text("buy milk".into()))]),
                None,
            )
            .unwrap();
        lifecore.add("Other", Body::default(), None).unwrap();
        lifecore
            .add("Note", Body::default(), Some(root.e))
            .unwrap();
        let aliases = BTreeMap::from([("root".into(), bounded_context(root.e))]);

        let historical = query_events(
            &lifecore,
            &aliases,
            &QueryPredicates {
                fresh_by: vec!["root".into()],
                type_name: vec!["Note".into()],
                match_text: vec!["milk$".into()],
            },
            QueryMode::Filter,
        )
        .unwrap();

        assert_eq!(historical.len(), 1);
        assert!(same_event(historical[0].ctx(), root.e));
    }

    #[test]
    fn body_parser_preserves_equals_in_text_value_and_rejects_duplicates() {
        let body = parse_body(&["equation=a=b".into()], &[]).unwrap();
        assert_eq!(body.0["equation"], Payload::Text("a=b".into()));
        assert!(parse_body(&["x=one".into()], &["x=1".into()]).is_err());
    }

    #[test]
    fn selections_are_one_based() {
        assert!(selected(&["a"], 0, "value").is_err());
        assert_eq!(*selected(&["a", "b"], 2, "value").unwrap(), "b");
        assert!(selected(&["a"], 2, "value").is_err());
    }

    #[test]
    fn singular_mutations_are_dry_run_by_default_and_committed_on_request() {
        let path = test_path("singular-mutation", "json");
        StateFile::empty().create(&path).unwrap();
        let add = |commit| {
            Command::Add(AddArgs {
                ty: "Note".into(),
                parent: None,
                alias: Some("note".into()),
                text: vec!["content=hello".into()],
                number: vec!["priority=2".into()],
                commit,
            })
        };

        cli(add(false)).execute_at(&path).unwrap();
        let loaded = StateFile::load(&path).unwrap();
        assert!(loaded.lifecore.last().is_none());
        assert!(loaded.cli.aliases.is_empty());

        cli(add(true)).execute_at(&path).unwrap();
        let loaded = StateFile::load(&path).unwrap();
        assert_eq!(pointed_index(loaded.lifecore.last().unwrap()), 0);
        assert!(loaded.cli.aliases.contains_key("note"));
        assert!(!loaded.lifecore.is_mock());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn init_registers_default_suite_and_refuses_existing_state() {
        let path = test_path("init", "json");

        cli(Command::Init).execute_at(&path).unwrap();

        let loaded = StateFile::load(&path).unwrap();
        let names = loaded
            .lifecore
            .current_analysis_suite()
            .0
            .into_iter()
            .map(|config| config.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                InteractiveFactory::<LifeCoreList>::name(),
                TypeCheckFactory::<LifeCoreList>::name()
            ]
        );
        assert!(loaded.lifecore.last().is_none());
        assert!(!loaded.lifecore.is_mock());
        let before = fs::read(&path).unwrap();
        assert!(cli(Command::Init).execute_at(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn detaching_does_not_read_sidecar_and_restores_state_bytes() {
        let state_path = test_path("detach-state", "json");
        let sidecar_path = test_path("detach-sidecar", "toml");
        StateFile::empty().create(&state_path).unwrap();
        fs::write(&sidecar_path, "this is not valid TOML = [").unwrap();
        let before = fs::read(&state_path).unwrap();

        cli(Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach {
                filename: sidecar_path.clone(),
            },
        }))
        .execute_at(&state_path)
        .unwrap();
        fs::remove_file(&sidecar_path).unwrap();
        cli(Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Detach,
        }))
        .execute_at(&state_path)
        .unwrap();

        assert_eq!(fs::read(&state_path).unwrap(), before);
        fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn sidecar_commit_promotes_alias_and_leaves_toml_file() {
        let state_path = test_path("sidecar-state", "json");
        let sidecar_path = test_path("sidecar-data", "toml");
        StateFile::empty().create(&state_path).unwrap();
        fs::write(
            &sidecar_path,
            r#"
                [[entry]]
                alias = "first"
                ty = "Note"
                [entry.body]
                content = "hello"

                [[entry]]
                alias = "second"
                ty = "Note"
                [entry.connect]
                connect_to = [{ target = "first", ty = "Related" }]
                connect_back_from = [{ target = "first", ty = "Back" }]
            "#,
        )
        .unwrap();
        cli(Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach {
                filename: sidecar_path.clone(),
            },
        }))
        .execute_at(&state_path)
        .unwrap();

        cli(Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Commit,
        }))
        .execute_at(&state_path)
        .unwrap();

        let loaded = StateFile::load(&state_path).unwrap();
        assert!(loaded.cli.sidecar.is_none());
        assert!(loaded.cli.aliases.contains_key("first"));
        assert!(loaded.cli.aliases.contains_key("second"));
        assert_eq!(pointed_index(loaded.lifecore.last().unwrap()), 3);
        let last_edge = loaded
            .lifecore
            .edge(loaded.lifecore.last().unwrap())
            .unwrap();
        assert_eq!(last_edge.ty_name, "Back");
        assert_eq!(pointed_index(last_edge.from), 0);
        assert_eq!(pointed_index(last_edge.to), 1);
        assert!(!loaded.lifecore.is_mock());
        assert!(sidecar_path.exists());
        fs::remove_file(state_path).unwrap();
        fs::remove_file(sidecar_path).unwrap();
    }

    #[test]
    fn attached_sidecar_blocks_singular_mutation_without_touching_state() {
        let state_path = test_path("restriction-state", "json");
        let sidecar_path = test_path("restriction-sidecar", "toml");
        StateFile::empty().create(&state_path).unwrap();
        fs::write(&sidecar_path, "entry = []\n").unwrap();
        cli(Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach {
                filename: sidecar_path.clone(),
            },
        }))
        .execute_at(&state_path)
        .unwrap();
        let attached = fs::read(&state_path).unwrap();

        let result = cli(Command::Add(AddArgs {
            ty: "Note".into(),
            parent: None,
            alias: None,
            text: vec![],
            number: vec![],
            commit: true,
        }))
        .execute_at(&state_path);

        assert!(result.is_err());
        assert_eq!(fs::read(&state_path).unwrap(), attached);
        fs::remove_file(state_path).unwrap();
        fs::remove_file(sidecar_path).unwrap();
    }

    #[test]
    fn analysis_inconsistency_rolls_back_committed_mutation_request() {
        let state_path = test_path("analysis-rollback", "json");
        let mut state = StateFile::empty();
        state
            .lifecore
            .add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "");
        state.create(&state_path).unwrap();
        let before = fs::read(&state_path).unwrap();

        let result = cli(Command::Add(AddArgs {
            ty: "Undefined".into(),
            parent: None,
            alias: Some("candidate".into()),
            text: vec![],
            number: vec![],
            commit: true,
        }))
        .execute_at(&state_path);

        assert!(result.is_err());
        assert_eq!(fs::read(&state_path).unwrap(), before);
        fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn missing_analyzer_factory_rolls_back_configuration() {
        let state_path = test_path("analyzer-rollback", "json");
        StateFile::empty().create(&state_path).unwrap();
        let before = fs::read(&state_path).unwrap();

        let result = cli(Command::Analyzer(AnalyzerArgs {
            command: AnalyzerCommand::Add {
                name: "Missing".into(),
                config: "config".into(),
                commit: true,
            },
        }))
        .execute_at(&state_path);

        assert!(result.is_err());
        assert_eq!(fs::read(&state_path).unwrap(), before);
        fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn applying_interactive_suggestion_commits_edge_and_barklists_message() {
        let state_path = test_path("interactive-state", "json");
        let mut state = StateFile::empty();
        let parent = state
            .lifecore
            .add("Thing", Body::default(), None)
            .unwrap();
        let other = state
            .lifecore
            .add("Other", Body::default(), None)
            .unwrap();
        state
            .lifecore
            .connect(parent.e, "Related", other.e)
            .unwrap();
        let child = state
            .lifecore
            .add("Thing", Body::default(), Some(parent.e))
            .unwrap();
        state
            .lifecore
            .add_analyzer(InteractiveFactory::<LifeCoreList>::name(), "");
        state.create(&state_path).unwrap();

        cli(Command::Suggestion(SuggestionArgs {
            command: SuggestionCommand::Apply {
                select: Some(1),
                alias: None,
                commit: true,
            },
        }))
        .execute_at(&state_path)
        .unwrap();

        let loaded = StateFile::load(&state_path).unwrap();
        let edge = loaded
            .lifecore
            .edge(loaded.lifecore.last().unwrap())
            .unwrap();
        assert_eq!(edge.ty_name, "Related");
        assert!(same_event(edge.from, child.e));
        assert!(same_event(edge.to, other.e));
        assert_eq!(loaded.cli.barklist.hashes.len(), 1);
        fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn bark_message_selection_persists_hash_without_changing_graph() {
        let state_path = test_path("bark-message", "json");
        let mut state = StateFile::empty();
        let parent = state
            .lifecore
            .add("Thing", Body::default(), None)
            .unwrap();
        let other = state
            .lifecore
            .add("Other", Body::default(), None)
            .unwrap();
        state
            .lifecore
            .connect(parent.e, "Related", other.e)
            .unwrap();
        state
            .lifecore
            .add("Thing", Body::default(), Some(parent.e))
            .unwrap();
        state
            .lifecore
            .add_analyzer(InteractiveFactory::<LifeCoreList>::name(), "");
        let last_before = state.lifecore.last().unwrap();
        state.create(&state_path).unwrap();

        cli(Command::Bark(BarkArgs {
            command: BarkCommand::Message { select: Some(1) },
        }))
        .execute_at(&state_path)
        .unwrap();

        let loaded = StateFile::load(&state_path).unwrap();
        assert_eq!(loaded.lifecore.last(), Some(last_before));
        assert_eq!(loaded.cli.barklist.hashes.len(), 1);
        let output = run_analysis(&loaded.lifecore).unwrap();
        assert!(
            visible_messages(output.0, &loaded.cli.barklist, false)
                .into_iter()
                .all(|message| !matches!(message, Message::Suggestion(_)))
        );
        fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn stale_alias_can_be_updated_to_selected_fresh_successor() {
        let state_path = test_path("alias-update", "json");
        let mut state = StateFile::empty();
        let root = state
            .lifecore
            .add("Note", Body::default(), None)
            .unwrap();
        let child = state
            .lifecore
            .add("Note", Body::default(), Some(root.e))
            .unwrap();
        state
            .cli
            .aliases
            .insert("note".into(), bounded_context(root.e));
        state.create(&state_path).unwrap();

        cli(Command::Alias(AliasArgs {
            command: AliasCommand::Update {
                name: "note".into(),
                select: Some(1),
            },
        }))
        .execute_at(&state_path)
        .unwrap();

        let loaded = StateFile::load(&state_path).unwrap();
        assert!(same_event(loaded.cli.aliases["note"], child.e));
        fs::remove_file(state_path).unwrap();
    }
}
