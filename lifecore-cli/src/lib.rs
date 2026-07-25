pub mod interactive_analyzer;
pub mod locator;
pub mod messages;
pub mod payload_input;
pub mod query;
pub mod sidecar;
pub mod state;

#[cfg(test)]
mod test;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    str::FromStr,
};

use anyhow::{Context as _, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use interactive_analyzer::InteractiveFactory;
use lifecore::{
    Context, Entry, LifeCore, LifeCoreList, Node, NodeQuery, NodeQueryExpression, OrderedSet,
    analysis::{AnalysisRunner, Message, Output, SuggestedAction},
    serialize::{LifeCoreSerializeExt, NodeBody},
    typing::{FieldDecl, Type},
};
use lifecore_typecheck::{TypeCheckFactory, types};
use locator::{DurableLocator, Locator};
use serde::Serialize;
use serde_json::{Value, json};
use state::{Dismissal, MESSAGE_ID_VERSION, StateFile};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Auto,
    Table,
    Lines,
    Json,
}

#[derive(Parser, Debug)]
#[command(version, about = "A typed, versioned graph database CLI")]
pub struct Cli {
    #[arg(short = 'i', long, global = true)]
    interactive: bool,
    #[arg(long, value_enum, default_value_t, global = true)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Init,
    Check(CheckArgs),
    Entry(EntryArgs),
    Query(QueryArgs),
    Alias(AliasArgs),
    Type(TypeArgs),
    Add(AddArgs),
    Connect(ConnectArgs),
    Message(MessageArgs),
    Analyzer(AnalyzerArgs),
    Sidecar(SidecarArgs),
    Graphviz(GraphvizArgs),
}

#[derive(Args, Debug)]
struct CheckArgs {
    #[arg(long, conflicts_with = "only_dismissed")]
    include_dismissed: bool,
    #[arg(long)]
    only_dismissed: bool,
    #[arg(long)]
    include_resolved: bool,
    #[arg(long, conflicts_with_all = ["include_dismissed", "only_dismissed", "include_resolved"])]
    dismissal_ids: bool,
}

#[derive(Args, Debug)]
struct EntryArgs {
    #[command(subcommand)]
    command: EntryCommand,
}

#[derive(Subcommand, Debug)]
enum EntryCommand {
    Show { locator: Locator },
}

#[derive(Args, Debug)]
struct QueryArgs {
    #[command(subcommand)]
    command: QueryCommand,
}

#[derive(Subcommand, Debug)]
enum QueryCommand {
    Nodes(QueryNodesArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum QueryCombinator {
    All,
    Any,
    None,
}

#[derive(Args, Debug)]
struct QueryNodesArgs {
    #[arg(value_enum)]
    combinator: QueryCombinator,
    #[arg(long = "where")]
    predicates: Vec<String>,
    #[arg(long)]
    at: Option<Locator>,
    #[arg(long, conflicts_with = "explain")]
    select: Option<Locator>,
    #[arg(long)]
    explain: Option<Locator>,
    #[arg(last = true, requires = "select")]
    action: Vec<String>,
}

#[derive(Args, Debug)]
struct AliasArgs {
    #[command(subcommand)]
    command: AliasCommand,
}

#[derive(Subcommand, Debug)]
enum AliasCommand {
    List {
        #[arg(long)]
        at: Option<Locator>,
    },
    Set {
        name: String,
        locator: Locator,
    },
    Remove {
        name: String,
    },
}

#[derive(Args, Debug)]
struct TypeArgs {
    #[command(subcommand)]
    command: TypeCommand,
}

#[derive(Subcommand, Debug)]
enum TypeCommand {
    List {
        #[arg(long)]
        at: Option<Locator>,
    },
    Show {
        locator: Locator,
    },
    Create(TypeCreateArgs),
    Override(TypeOverrideArgs),
}

#[derive(Args, Debug, Clone)]
struct TypeCreateArgs {
    name: String,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long)]
    outgoing: Vec<String>,
    #[arg(long)]
    incoming: Vec<String>,
    #[arg(long = "allows-override")]
    allows_override: Vec<String>,
    #[command(flatten)]
    commit: CommitArgs,
}

#[derive(Args, Debug, Clone)]
struct TypeOverrideArgs {
    name: String,
    #[arg(long)]
    at: Option<Locator>,
    #[arg(long = "as")]
    new_name: Option<String>,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long)]
    drop_field: Vec<String>,
    #[arg(long)]
    allow_outgoing: Vec<String>,
    #[arg(long)]
    disallow_outgoing: Vec<String>,
    #[arg(long)]
    allow_incoming: Vec<String>,
    #[arg(long)]
    disallow_incoming: Vec<String>,
    #[arg(long = "allows-override")]
    allows_override: Vec<String>,
    #[arg(long)]
    disallow_override: Vec<String>,
    #[command(flatten)]
    commit: CommitArgs,
}

#[derive(Args, Debug)]
struct AddArgs {
    ty: String,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long)]
    parent: Option<String>,
    #[command(flatten)]
    commit: CommitArgs,
}

#[derive(Args, Debug)]
struct ConnectArgs {
    #[arg(long)]
    from: String,
    #[arg(long = "type")]
    ty: String,
    #[arg(long)]
    to: String,
    #[command(flatten)]
    commit: CommitArgs,
}

#[derive(Args, Debug, Clone)]
struct CommitArgs {
    #[arg(long)]
    commit: bool,
    #[arg(long, value_name = "ENTRY-LOCATOR|EMPTY")]
    expect_head: Option<String>,
}

#[derive(Args, Debug)]
struct MessageArgs {
    #[command(subcommand)]
    command: MessageCommand,
}

#[derive(Subcommand, Debug)]
enum MessageCommand {
    Show {
        id: String,
    },
    Dismiss {
        id: String,
    },
    Restore {
        id: String,
    },
    Apply {
        id: String,
        #[arg(long)]
        keep_message: bool,
        #[command(flatten)]
        commit: CommitArgs,
    },
}

#[derive(Args, Debug)]
struct AnalyzerArgs {
    #[command(subcommand)]
    command: AnalyzerCommand,
}

#[derive(Subcommand, Debug)]
enum AnalyzerCommand {
    List,
    Add {
        name: String,
        config: String,
        #[command(flatten)]
        commit: CommitArgs,
    },
}

#[derive(Args, Debug)]
struct SidecarArgs {
    #[command(subcommand)]
    command: SidecarCommand,
}

#[derive(Subcommand, Debug)]
enum SidecarCommand {
    Attach {
        path: PathBuf,
    },
    Detach,
    Dump,
    Commit {
        #[arg(long, value_name = "ENTRY-LOCATOR|EMPTY")]
        expect_head: Option<String>,
    },
}

#[derive(Args, Debug)]
struct GraphvizArgs {
    target: PathBuf,
    #[arg(long)]
    at: Option<Locator>,
    #[arg(long)]
    full: bool,
    #[arg(long)]
    fields: bool,
    #[arg(long)]
    dot: bool,
}

#[derive(Clone)]
struct Prepared {
    state: StateFile,
    aliases: BTreeMap<String, DurableLocator>,
    temporary_aliases: BTreeSet<String>,
    added: Vec<usize>,
    analysis: Output,
    inconsistent: bool,
}

impl Cli {
    pub fn run() -> Result<()> {
        Self::parse().execute()
    }

    pub fn execute(self) -> Result<()> {
        self.execute_at(&state::state_path()?)
    }

    pub fn execute_at(self, path: &Path) -> Result<()> {
        if self.interactive {
            if self.command.is_some() {
                bail!("-i cannot be combined with a batch command");
            }
            if self.format == OutputFormat::Json {
                bail!("-i cannot be combined with JSON output");
            }
            return interactive(path, self.format);
        }
        let command = self
            .command
            .ok_or_else(|| anyhow!("a command is required"))?;
        if matches!(command, Command::Init) {
            return initialize(path, self.format);
        }
        let state = StateFile::load(path)?;
        match command {
        Command::Init => unreachable!(),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Detach,
        }) => detach_sidecar(path, state),
        Command::Sidecar(SidecarArgs {
            command: SidecarCommand::Attach { path: sidecar },
        }) => attach_sidecar(path, state, &sidecar),
        command => execute_loaded(path, state, command, self.format),
        }
    }
}

fn initialize(path: &Path, format: OutputFormat) -> Result<()> {
    let mut state = StateFile::new();
    state
        .lifecore
        .add_analyzer(InteractiveFactory::<LifeCoreList>::name(), "");
    state
        .lifecore
        .add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "");
    state
        .lifecore
        .basic_validation()
        .map_err(|failure| anyhow!("initial graph failed basic validation: {failure:?}"))?;
    let analysis = run_analysis(&state.lifecore)?;
    render_messages(
        &state,
        &analysis,
        state::latest_view(&state.lifecore),
        format,
        None,
    )?;
    if analysis.has_error() {
        bail!("initial graph failed analysis");
    }
    state.lifecore.apply()?;
    state.create(path)?;
    eprintln!("Initialized {}", path.display());
    Ok(())
}

fn execute_loaded(
    path: &Path,
    state: StateFile,
    command: Command,
    format: OutputFormat,
) -> Result<()> {
    if state.cli.sidecar.is_some() && command_forbidden_with_sidecar(&command) {
        bail!("this operation is unavailable while a sidecar is attached");
    }
    let prepared = prepare(state)?;
    if prepared.inconsistent
        && !matches!(
            command,
            Command::Check(_)
                | Command::Sidecar(SidecarArgs {
                    command: SidecarCommand::Commit { .. }
                })
        )
    {
        bail!("attached sidecar is inconsistent; use check or sidecar detach");
    }
    match command {
    Command::Check(args) => check(&prepared, args, format),
    Command::Entry(args) => entry_command(&prepared, args, format),
    Command::Query(args) => query_command(path, prepared, args, format),
    Command::Alias(args) => alias_command(path, prepared, args, format),
    Command::Type(args) => type_command(path, prepared, args, format),
    Command::Add(args) => add_command(path, prepared, args, format),
    Command::Connect(args) => connect_command(path, prepared, args, format),
    Command::Message(args) => message_command(path, prepared, args, format),
    Command::Analyzer(args) => analyzer_command(path, prepared, args, format),
    Command::Sidecar(SidecarArgs {
        command: SidecarCommand::Dump,
    }) => sidecar_dump(&prepared, format),
    Command::Sidecar(SidecarArgs {
        command: SidecarCommand::Commit { expect_head },
    }) => sidecar_commit(path, prepared, expect_head, format),
    Command::Graphviz(args) => graphviz(&prepared, args),
    Command::Init
    | Command::Sidecar(SidecarArgs {
        command: SidecarCommand::Attach { .. } | SidecarCommand::Detach,
    }) => unreachable!(),
    }
}

fn command_forbidden_with_sidecar(command: &Command) -> bool {
    matches!(
        command,
        Command::Add(_)
            | Command::Connect(_)
            | Command::Type(TypeArgs {
                command: TypeCommand::Create(_) | TypeCommand::Override(_)
            })
            | Command::Message(MessageArgs {
                command: MessageCommand::Dismiss { .. }
                    | MessageCommand::Restore { .. }
                    | MessageCommand::Apply { .. }
            })
            | Command::Analyzer(AnalyzerArgs {
                command: AnalyzerCommand::Add { .. }
            })
            | Command::Alias(AliasArgs {
                command: AliasCommand::Set { .. } | AliasCommand::Remove { .. }
            })
    )
}

fn run_analysis(lifecore: &LifeCoreList) -> Result<Output> {
    let mut runner = AnalysisRunner::default();
    runner.register_analyzer_kind(Box::new(InteractiveFactory::<LifeCoreList>::default()))?;
    runner.register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))?;
    Ok(runner.run(lifecore)?)
}

fn prepare(mut state: StateFile) -> Result<Prepared> {
    let mut aliases = state.cli.aliases.clone();
    let mut temporary_aliases = BTreeSet::new();
    let mut added = Vec::new();
    if let Some(path) = state.cli.sidecar.clone() {
        let sidecar = sidecar::Sidecar::load(&path)?;
        for entry in sidecar.entries {
            let view_before = state::latest_view(&state.lifecore);
            let captured_head = locator::latest_node(&state.lifecore, view_before);
            let parent = entry
                .parent
                .as_deref()
                .map(|reference| {
                    resolve_node_ref(
                        state.cli.state_uuid,
                        &state.lifecore,
                        &aliases,
                        reference,
                        view_before,
                        captured_head.as_ref(),
                    )
                })
                .transpose()?;
            let body = payload_input::body(&entry.body)?;
            let node = state
                .lifecore
                .add_mock(&entry.ty, body, parent.map(|node| node.e))?;
            added.push(node.e.entry().0);
            if let Some(name) = entry.alias {
                locator::validate_alias(&name)?;
                if aliases.contains_key(&name) {
                    bail!("sidecar alias {name:?} shadows an existing alias");
                }
                aliases.insert(
                    name.clone(),
                    locator::durable(state.cli.state_uuid, &state.lifecore, node.e.entry().0)?,
                );
                temporary_aliases.insert(name);
            }
            for connection in entry.connect.connect_to {
                let view = state::latest_view(&state.lifecore);
                let target = resolve_node_ref(
                    state.cli.state_uuid,
                    &state.lifecore,
                    &aliases,
                    &connection.target,
                    view,
                    captured_head.as_ref(),
                )?;
                let edge = state
                    .lifecore
                    .connect_mock(node.e, &connection.ty, target.e)?;
                added.push(edge.e.entry().0);
            }
            for connection in entry.connect.connect_back_from {
                let view = state::latest_view(&state.lifecore);
                let target = resolve_node_ref(
                    state.cli.state_uuid,
                    &state.lifecore,
                    &aliases,
                    &connection.target,
                    view,
                    captured_head.as_ref(),
                )?;
                let edge = state
                    .lifecore
                    .connect_mock(target.e, &connection.ty, node.e)?;
                added.push(edge.e.entry().0);
            }
        }
    }
    let analysis = run_analysis(&state.lifecore)?;
    let inconsistent = analysis.has_error();
    Ok(Prepared {
        state,
        aliases,
        temporary_aliases,
        added,
        analysis,
        inconsistent,
    })
}

fn resolve_node_ref(
    uuid: uuid::Uuid,
    lifecore: &LifeCoreList,
    aliases: &BTreeMap<String, DurableLocator>,
    reference: &str,
    view: Context,
    captured_head: Option<&Node>,
) -> Result<Node> {
    if reference == "HEAD" {
        return captured_head
            .cloned()
            .or_else(|| locator::latest_node(lifecore, view))
            .ok_or_else(|| anyhow!("HEAD cannot resolve because the graph has no nodes"));
    }
    let entry = if reference.contains(':') {
        let locator = Locator::from_str(reference)?;
        locator::resolve(uuid, lifecore, &locator, view)?.0
    } else {
        locator::validate_alias(reference)?;
        let locator = aliases
            .get(reference)
            .ok_or_else(|| anyhow!("unknown alias {reference:?}"))?;
        locator::resolve_durable(uuid, lifecore, locator, view)?.0
    };
    match entry {
    Entry::Node(node) => Ok(node),
    Entry::Edge(_) => bail!("entry is not a node"),
    }
}

fn view_from_at(prepared: &Prepared, at: Option<&Locator>) -> Result<Context> {
    let latest = state::latest_view(&prepared.state.lifecore);
    match at {
    Some(locator) => {
        let (entry, _) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            locator,
            latest,
        )?;
        Ok(entry.entry_context().shrink_to_inclusive_left())
    }
    None => Ok(latest),
    }
}

fn attach_sidecar(path: &Path, mut state: StateFile, sidecar: &Path) -> Result<()> {
    if state.cli.sidecar.is_some() {
        bail!("a sidecar is already attached; detach it first");
    }
    state.cli.sidecar = Some(state::absolute_existing_path(sidecar)?);
    state.save(path)
}

fn detach_sidecar(path: &Path, mut state: StateFile) -> Result<()> {
    if state.cli.sidecar.take().is_none() {
        bail!("no sidecar is attached");
    }
    state.save(path)
}

fn check(prepared: &Prepared, args: CheckArgs, format: OutputFormat) -> Result<()> {
    if args.dismissal_ids {
        let emitted = messages::universe(prepared.analysis.0.clone())
            .into_iter()
            .map(|value| value.digest)
            .collect::<BTreeSet<_>>();
        let items = prepared
            .state
            .cli
            .dismissals
            .iter()
            .map(|value| {
                json!({
                    "full_id": format!("m:{}", value.digest),
                    "emitted": emitted.contains(&value.digest),
                    "version": value.version,
                })
            })
            .collect::<Vec<_>>();
        return render_items(None, &items, format);
    }
    let view = state::latest_view(&prepared.state.lifecore);
    render_messages(
        &prepared.state,
        &prepared.analysis,
        view,
        format,
        Some(&args),
    )
}

fn render_messages(
    state: &StateFile,
    analysis: &Output,
    view: Context,
    format: OutputFormat,
    filter: Option<&CheckArgs>,
) -> Result<()> {
    let universe = messages::universe(analysis.0.clone());
    let mut records = messages::records(&universe, &state.cli.dismissals, &state.lifecore, view);
    if let Some(filter) = filter {
        records.retain(|record| {
            if record.severity == "Inconsistency" {
                return true;
            }
            let dismissal_match = if filter.only_dismissed {
                record.dismissed
            } else if filter.include_dismissed {
                true
            } else {
                !record.dismissed
            };
            let resolution_match = filter.include_resolved || !record.resolved;
            dismissal_match && resolution_match
        });
    }
    render_items(Some(view_value(state, view)?), &records, format)
}

fn entry_command(prepared: &Prepared, args: EntryArgs, format: OutputFormat) -> Result<()> {
    match args.command {
    EntryCommand::Show { locator } => {
        let view = state::latest_view(&prepared.state.lifecore);
        let (entry, durable) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            &locator,
            view,
        )?;
        let value = entry_value(prepared, entry, durable, view)?;
        render_items(Some(view_value(&prepared.state, view)?), &[value], format)
    }
    }
}

fn query_command(
    path: &Path,
    mut prepared: Prepared,
    args: QueryArgs,
    format: OutputFormat,
) -> Result<()> {
    let QueryCommand::Nodes(args) = args.command;
    let view = view_from_at(&prepared, args.at.as_ref())?;
    let predicates = args
        .predicates
        .iter()
        .map(|source| query::predicate(source))
        .collect::<Result<Vec<_>>>()?;
    let expression = match args.combinator {
    QueryCombinator::All => NodeQueryExpression::All(predicates),
    QueryCombinator::Any => NodeQueryExpression::Any(predicates),
    QueryCombinator::None => NodeQueryExpression::None(predicates),
    };
    let query = NodeQuery {
        context: view,
        expression,
    };
    let results = prepared.state.lifecore.query(query.clone());
    if let Some(explain) = args.explain {
        let (entry, durable) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            &explain,
            view,
        )?;
        let Entry::Node(node) = entry else {
            bail!("entry is not a node");
        };
        let explanation = prepared
            .state
            .lifecore
            .explain_query(&query, node.e)
            .ok_or_else(|| anyhow!("entry is outside the selected view"))?;
        let item = json!({
            "locator": durable.to_string(),
            "full_locator": format!("{}:{}", durable.index, durable.digest),
            "predicates": args.predicates.iter().zip(explanation.predicate_matches).map(
                |(source, matched)| json!({"predicate": source, "matched": matched})
            ).collect::<Vec<_>>(),
            "matches": explanation.matches,
        });
        return render_items(Some(view_value(&prepared.state, view)?), &[item], format);
    }
    if let Some(select) = args.select {
        let (entry, durable) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            &select,
            view,
        )?;
        let Entry::Node(node) = entry else {
            bail!("entry is not a node");
        };
        if !results
            .iter()
            .any(|candidate| candidate.e.entry() == node.e.entry())
        {
            bail!("selected entry was not returned by query");
        }
        if args.action.is_empty() {
            let item = entry_value(&prepared, Entry::Node(node), durable, view)?;
            return render_items(Some(view_value(&prepared.state, view)?), &[item], format);
        }
        return selected_action(path, &mut prepared, node, &args.action, format);
    }
    let items = results
        .into_iter()
        .map(|node| {
            let durable = locator::durable(
                prepared.state.cli.state_uuid,
                &prepared.state.lifecore,
                node.e.entry().0,
            )?;
            entry_value(&prepared, Entry::Node(node), durable, view)
        })
        .collect::<Result<Vec<_>>>()?;
    render_items(Some(view_value(&prepared.state, view)?), &items, format)
}

fn selected_action(
    path: &Path,
    prepared: &mut Prepared,
    selected: Node,
    action: &[String],
    format: OutputFormat,
) -> Result<()> {
    match action {
    [entry, inspect] if entry == "entry" && inspect == "inspect" => {
        let view = selected.e.window();
        let durable = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            selected.e.entry().0,
        )?;
        let item = entry_value(prepared, Entry::Node(selected), durable, view)?;
        render_items(Some(view_value(&prepared.state, view)?), &[item], format)
    }
    [alias, set, name] if alias == "alias" && set == "set" => {
        locator::validate_alias(name)?;
        let durable = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            selected.e.entry().0,
        )?;
        prepared.state.cli.aliases.insert(name.to_string(), durable);
        prepared.state.save(path)
    }
    _ if action.first().is_some_and(|value| value == "add-child") => {
        let parsed = SelectedAdd::try_parse_from(
            std::iter::once("add-child".to_string()).chain(action.iter().skip(1).cloned()),
        )?;
        let body = payload_input::assignments(&parsed.fields)?;
        let mut candidate = prepared.state.clone();
        candidate
            .lifecore
            .add_mock(&parsed.ty, body, Some(selected.e))?;
        finish_graph(path, candidate, parsed.commit, format, None)
    }
    _ if action.len() >= 2 && action[0] == "type" && action[1] == "override" => {
        if selected.ty_name != Type::TYPE_NAME
            || prepared.state.lifecore.is_stale(selected.e) != Some(false)
        {
            bail!("selected node is not a current valid Type declaration");
        }
        let source = Type::try_from_body(&selected.field)
            .map_err(|_| anyhow!("selected node is not a valid Type declaration"))?;
        let parsed = SelectedTypeOverride::try_parse_from(
            std::iter::once("type-override".to_string()).chain(action.iter().skip(2).cloned()),
        )?;
        validate_commit(&parsed.commit)?;
        let override_args = TypeOverrideArgs {
            name: source.name.clone(),
            at: None,
            new_name: parsed.new_name,
            fields: parsed.fields,
            drop_field: parsed.drop_field,
            allow_outgoing: parsed.allow_outgoing,
            disallow_outgoing: parsed.disallow_outgoing,
            allow_incoming: parsed.allow_incoming,
            disallow_incoming: parsed.disallow_incoming,
            allows_override: parsed.allows_override,
            disallow_override: parsed.disallow_override,
            commit: CommitArgs {
                commit: parsed.commit.commit,
                expect_head: parsed.commit.expect_head.clone(),
            },
        };
        let declaration = patch_type(&source, &override_args)?;
        let mut candidate = prepared.state.clone();
        candidate
            .lifecore
            .add_typed_mock(declaration, Some(selected.e))?;
        finish_graph(path, candidate, parsed.commit, format, None)
    }
    _ => bail!("unknown query selection action"),
    }
}

#[derive(Parser)]
struct SelectedAdd {
    ty: String,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[command(flatten)]
    commit: CommitArgs,
}

#[derive(Parser)]
struct SelectedTypeOverride {
    #[arg(long = "as")]
    new_name: Option<String>,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long)]
    drop_field: Vec<String>,
    #[arg(long)]
    allow_outgoing: Vec<String>,
    #[arg(long)]
    disallow_outgoing: Vec<String>,
    #[arg(long)]
    allow_incoming: Vec<String>,
    #[arg(long)]
    disallow_incoming: Vec<String>,
    #[arg(long = "allows-override")]
    allows_override: Vec<String>,
    #[arg(long)]
    disallow_override: Vec<String>,
    #[command(flatten)]
    commit: CommitArgs,
}

fn alias_command(
    path: &Path,
    mut prepared: Prepared,
    args: AliasArgs,
    format: OutputFormat,
) -> Result<()> {
    match args.command {
    AliasCommand::List { at } => {
        let view = view_from_at(&prepared, at.as_ref())?;
        let visible = prepared
            .state
            .lifecore
            .window(view.items())
            .map(|entry| entry.entry_context().entry().0)
            .collect::<BTreeSet<_>>();
        let items = prepared
            .aliases
            .iter()
            .map(|(name, durable)| {
                let status = if !visible.contains(&durable.index) {
                    "outside-view"
                } else {
                    let context =
                        locator::context_in_view(&prepared.state.lifecore, durable.index, view)
                            .unwrap();
                    if prepared.state.lifecore.is_stale(context) == Some(true) {
                        "stale"
                    } else {
                        "current"
                    }
                };
                json!({
                    "name": name,
                    "locator": durable.to_string(),
                    "full_locator": format!("{}:{}", durable.index, durable.digest),
                    "status": status,
                    "temporary": prepared.temporary_aliases.contains(name),
                })
            })
            .collect::<Vec<_>>();
        render_items(Some(view_value(&prepared.state, view)?), &items, format)
    }
    AliasCommand::Set { name, locator } => {
        locator::validate_alias(&name)?;
        let view = state::latest_view(&prepared.state.lifecore);
        let (entry, durable) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            &locator,
            view,
        )?;
        if !matches!(entry, Entry::Node(_)) {
            bail!("entry is not a node");
        }
        prepared.state.cli.aliases.insert(name, durable);
        prepared.state.save(path)
    }
    AliasCommand::Remove { name } => {
        locator::validate_alias(&name)?;
        if prepared.state.cli.aliases.remove(&name).is_none() {
            bail!("unknown alias {name:?}");
        }
        prepared.state.save(path)
    }
    }
}

fn add_command(path: &Path, prepared: Prepared, args: AddArgs, format: OutputFormat) -> Result<()> {
    validate_commit(&args.commit)?;
    let view = state::latest_view(&prepared.state.lifecore);
    let parent = args
        .parent
        .as_deref()
        .map(|reference| {
            resolve_node_ref(
                prepared.state.cli.state_uuid,
                &prepared.state.lifecore,
                &prepared.aliases,
                reference,
                view,
                None,
            )
        })
        .transpose()?;
    let body = payload_input::assignments(&args.fields)?;
    let mut candidate = prepared.state;
    candidate
        .lifecore
        .add_mock(&args.ty, body, parent.map(|node| node.e))?;
    finish_graph(path, candidate, args.commit, format, None)
}

fn connect_command(
    path: &Path,
    prepared: Prepared,
    args: ConnectArgs,
    format: OutputFormat,
) -> Result<()> {
    validate_commit(&args.commit)?;
    let view = state::latest_view(&prepared.state.lifecore);
    let from = resolve_node_ref(
        prepared.state.cli.state_uuid,
        &prepared.state.lifecore,
        &prepared.aliases,
        &args.from,
        view,
        None,
    )?;
    let to = resolve_node_ref(
        prepared.state.cli.state_uuid,
        &prepared.state.lifecore,
        &prepared.aliases,
        &args.to,
        view,
        None,
    )?;
    let mut candidate = prepared.state;
    candidate.lifecore.connect_mock(from.e, &args.ty, to.e)?;
    finish_graph(path, candidate, args.commit, format, None)
}

fn type_command(
    path: &Path,
    prepared: Prepared,
    args: TypeArgs,
    format: OutputFormat,
) -> Result<()> {
    match args.command {
    TypeCommand::List { at } => {
        let view = view_from_at(&prepared, at.as_ref())?;
        let mut declarations = types(&prepared.state.lifecore, view).collect::<Vec<_>>();
        declarations.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.0.entry().cmp(&right.1.0.entry()))
        });
        let items = declarations
            .into_iter()
            .map(|(name, (context, declaration))| {
                let durable = locator::durable(
                    prepared.state.cli.state_uuid,
                    &prepared.state.lifecore,
                    context.entry().0,
                )?;
                Ok(json!({
                    "name": name,
                    "locator": durable.to_string(),
                    "full_locator": format!("{}:{}", durable.index, durable.digest),
                    "declaration": type_value(&declaration),
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        render_items(Some(view_value(&prepared.state, view)?), &items, format)
    }
    TypeCommand::Show { locator: supplied } => {
        let view = state::latest_view(&prepared.state.lifecore);
        let (entry, durable) = locator::resolve(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            &supplied,
            view,
        )?;
        let Entry::Node(node) = entry else {
            bail!("entry is not a node");
        };
        if node.ty_name != Type::TYPE_NAME {
            bail!("selected node is not a valid Type declaration");
        }
        let declaration = Type::try_from_body(&node.field)
            .map_err(|_| anyhow!("selected node is not a valid Type declaration"))?;
        render_items(
            Some(view_value(&prepared.state, view)?),
            &[json!({
                "locator": durable.to_string(),
                "full_locator": format!("{}:{}", durable.index, durable.digest),
                "declaration": type_value(&declaration),
            })],
            format,
        )
    }
    TypeCommand::Create(args) => {
        validate_commit(&args.commit)?;
        let declaration = type_create(&args)?;
        let mut candidate = prepared.state;
        candidate.lifecore.add_typed_mock(declaration, None)?;
        finish_graph(path, candidate, args.commit, format, None)
    }
    TypeCommand::Override(args) => {
        validate_commit(&args.commit)?;
        let view = view_from_at(&prepared, args.at.as_ref())?;
        let matches = types(&prepared.state.lifecore, view)
            .filter(|(name, _)| name == &args.name)
            .collect::<Vec<_>>();
        let [(_, (parent, source))] = matches.as_slice() else {
            bail!(
                "type override requires exactly one active declaration named {:?}",
                args.name
            );
        };
        let declaration = patch_type(source, &args)?;
        let mut candidate = prepared.state;
        candidate
            .lifecore
            .add_typed_mock(declaration, Some(*parent))?;
        finish_graph(path, candidate, args.commit, format, None)
    }
    }
}

fn type_create(args: &TypeCreateArgs) -> Result<Type> {
    let fields = unique_fields(&args.fields)?;
    unique_strings(&args.outgoing, "outgoing permission")?;
    unique_strings(&args.incoming, "incoming permission")?;
    unique_strings(&args.allows_override, "override permission")?;
    Ok(Type {
        name: args.name.clone(),
        body: fields.into_iter().collect(),
        outgoing: args.outgoing.clone().into(),
        incoming: args.incoming.clone().into(),
        additional_override: args.allows_override.clone().into(),
    })
}

fn patch_type(source: &Type, args: &TypeOverrideArgs) -> Result<Type> {
    let replacements = unique_fields(&args.fields)?;
    let replacement_names = replacements
        .iter()
        .map(|field| field.name().to_string())
        .collect::<BTreeSet<_>>();
    unique_strings(&args.drop_field, "dropped field")?;
    if args
        .drop_field
        .iter()
        .any(|name| replacement_names.contains(name))
    {
        bail!("a field cannot be replaced and dropped in one patch");
    }
    conflicts(
        &args.allow_outgoing,
        &args.disallow_outgoing,
        "outgoing permission",
    )?;
    conflicts(
        &args.allow_incoming,
        &args.disallow_incoming,
        "incoming permission",
    )?;
    conflicts(
        &args.allows_override,
        &args.disallow_override,
        "override permission",
    )?;
    let replacement_map = replacements
        .iter()
        .map(|field| (field.name().to_string(), field.clone()))
        .collect::<BTreeMap<_, _>>();
    let drops = args.drop_field.iter().collect::<BTreeSet<_>>();
    let mut fields = Vec::new();
    for field in source.body.iter() {
        if drops.contains(&field.name().to_string()) {
            continue;
        }
        fields.push(
            replacement_map
                .get(field.name())
                .cloned()
                .unwrap_or_else(|| field.clone()),
        );
    }
    for field in replacements {
        if !source.body.iter().any(|old| old.name() == field.name()) {
            fields.push(field);
        }
    }
    let mut result = source.clone();
    result.name = args.new_name.clone().unwrap_or_else(|| source.name.clone());
    result.body = fields.into_iter().collect();
    patch_set(
        &mut result.outgoing,
        &args.allow_outgoing,
        &args.disallow_outgoing,
    );
    patch_set(
        &mut result.incoming,
        &args.allow_incoming,
        &args.disallow_incoming,
    );
    patch_set(
        &mut result.additional_override,
        &args.allows_override,
        &args.disallow_override,
    );
    if &result == source {
        bail!("type override patch is a no-op");
    }
    Ok(result)
}

fn unique_fields(values: &[String]) -> Result<Vec<FieldDecl>> {
    let fields = values
        .iter()
        .map(|value| {
            FieldDecl::from_str(value)
                .map_err(|_| anyhow!("field declaration syntax invalid: {value:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut names = BTreeSet::new();
    for field in &fields {
        if !names.insert(field.name()) {
            bail!("field {:?} is edited more than once", field.name());
        }
    }
    Ok(fields)
}

fn unique_strings(values: &[String], kind: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            bail!("{kind} {value:?} is repeated");
        }
    }
    Ok(())
}

fn conflicts(allow: &[String], disallow: &[String], kind: &str) -> Result<()> {
    unique_strings(allow, kind)?;
    unique_strings(disallow, kind)?;
    if allow.iter().any(|value| disallow.contains(value)) {
        bail!("{kind} cannot be both allowed and disallowed");
    }
    Ok(())
}

fn patch_set(set: &mut OrderedSet<String>, allow: &[String], disallow: &[String]) {
    for value in disallow {
        set.remove(value);
    }
    for value in allow {
        set.insert(value.clone());
    }
}

fn message_command(
    path: &Path,
    mut prepared: Prepared,
    args: MessageArgs,
    format: OutputFormat,
) -> Result<()> {
    let universe = messages::universe(prepared.analysis.0.clone());
    let view = state::latest_view(&prepared.state.lifecore);
    match args.command {
    MessageCommand::Show { id } => {
        let selected = messages::resolve(&universe, &id)?;
        let records = messages::records(
            std::slice::from_ref(selected),
            &prepared.state.cli.dismissals,
            &prepared.state.lifecore,
            view,
        );
        render_items(Some(view_value(&prepared.state, view)?), &records, format)
    }
    MessageCommand::Dismiss { id } => {
        let selected = messages::resolve(&universe, &id)?;
        if matches!(selected.message, Message::Inconsistency(_)) {
            bail!("an Inconsistency cannot be dismissed");
        }
        if !messages::is_dismissed(&prepared.state.cli.dismissals, &selected.digest) {
            prepared.state.cli.dismissals.push(Dismissal {
                version: MESSAGE_ID_VERSION,
                digest: selected.digest.clone(),
            });
            prepared
                .state
                .cli
                .dismissals
                .sort_by(|left, right| left.digest.cmp(&right.digest));
            prepared.state.save(path)?;
        }
        Ok(())
    }
    MessageCommand::Restore { id } => {
        let prefix = messages::parse_handle(&id)?;
        let mut candidates = universe
            .iter()
            .map(|value| value.digest.as_str())
            .chain(
                prepared
                    .state
                    .cli
                    .dismissals
                    .iter()
                    .map(|value| value.digest.as_str()),
            )
            .filter(|digest| digest.starts_with(prefix))
            .collect::<BTreeSet<_>>();
        let digest = match candidates.len() {
        0 => bail!("unknown message ID {id}"),
        1 => candidates.pop_first().unwrap().to_string(),
        _ => bail!("message ID is ambiguous"),
        };
        let before = prepared.state.cli.dismissals.len();
        prepared
            .state
            .cli
            .dismissals
            .retain(|value| value.digest != digest);
        if before == prepared.state.cli.dismissals.len() {
            bail!("message is not dismissed");
        }
        prepared.state.save(path)
    }
    MessageCommand::Apply {
        id,
        keep_message,
        commit,
    } => {
        validate_commit(&commit)?;
        let selected = messages::resolve(&universe, &id)?;
        let Message::Suggestion(suggestion) = &selected.message else {
            bail!("only a Suggestion can be applied");
        };
        let mut candidate = prepared.state;
        for action in &suggestion.action {
            match action {
            SuggestedAction::AddNode {
                ty_name,
                payload,
                parent,
            } => {
                let view = state::latest_view(&candidate.lifecore);
                let parent = parent
                    .map(|index| {
                        locator::context_in_view(&candidate.lifecore, index.0, view)
                            .ok_or_else(|| anyhow!("suggested parent is unavailable"))
                    })
                    .transpose()?;
                candidate
                    .lifecore
                    .add_mock(ty_name, payload.clone(), parent)?;
            }
            SuggestedAction::AddEdge { from, ty_name, to } => {
                let view = state::latest_view(&candidate.lifecore);
                let from = locator::context_in_view(&candidate.lifecore, from.0, view)
                    .ok_or_else(|| anyhow!("suggested edge endpoint is unavailable"))?;
                let to = locator::context_in_view(&candidate.lifecore, to.0, view)
                    .ok_or_else(|| anyhow!("suggested edge endpoint is unavailable"))?;
                candidate.lifecore.connect_mock(from, ty_name, to)?;
            }
            }
        }
        finish_graph(
            path,
            candidate,
            commit,
            format,
            (!keep_message).then_some(selected.digest.clone()),
        )
    }
    }
}

fn analyzer_command(
    path: &Path,
    prepared: Prepared,
    args: AnalyzerArgs,
    format: OutputFormat,
) -> Result<()> {
    match args.command {
    AnalyzerCommand::List => {
        let items = prepared
            .state
            .lifecore
            .current_analysis_suite()
            .0
            .into_iter()
            .map(|value| json!({"name": value.name, "config": value.config}))
            .collect::<Vec<_>>();
        render_items(
            Some(view_value(
                &prepared.state,
                state::latest_view(&prepared.state.lifecore),
            )?),
            &items,
            format,
        )
    }
    AnalyzerCommand::Add {
        name,
        config,
        commit,
    } => {
        validate_commit(&commit)?;
        let mut candidate = prepared.state;
        candidate.lifecore.add_analyzer(&name, &config);
        finish_graph(path, candidate, commit, format, None)
    }
    }
}

fn sidecar_dump(prepared: &Prepared, format: OutputFormat) -> Result<()> {
    if prepared.state.cli.sidecar.is_none() {
        bail!("no sidecar is attached");
    }
    let view = state::latest_view(&prepared.state.lifecore);
    let items = prepared
        .added
        .iter()
        .map(|index| {
            let context = locator::context_in_view(&prepared.state.lifecore, *index, view).unwrap();
            let entry = prepared.state.lifecore.get(context).unwrap();
            let durable = locator::durable(
                prepared.state.cli.state_uuid,
                &prepared.state.lifecore,
                *index,
            )?;
            entry_value(prepared, entry, durable, view)
        })
        .collect::<Result<Vec<_>>>()?;
    render_items(Some(view_value(&prepared.state, view)?), &items, format)
}

fn sidecar_commit(
    path: &Path,
    mut prepared: Prepared,
    expect_head: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    if prepared.state.cli.sidecar.is_none() {
        bail!("no sidecar is attached");
    }
    if prepared.inconsistent {
        render_messages(
            &prepared.state,
            &prepared.analysis,
            state::latest_view(&prepared.state.lifecore),
            format,
            None,
        )?;
        bail!("sidecar analysis contains an Inconsistency");
    }
    for name in &prepared.temporary_aliases {
        prepared
            .state
            .cli
            .aliases
            .insert(name.clone(), prepared.aliases[name].clone());
    }
    prepared.state.cli.sidecar = None;
    finish_graph(
        path,
        prepared.state,
        CommitArgs {
            commit: true,
            expect_head,
        },
        format,
        None,
    )
}

fn finish_graph(
    path: &Path,
    mut candidate: StateFile,
    commit: CommitArgs,
    format: OutputFormat,
    dismiss_on_commit: Option<String>,
) -> Result<()> {
    validate_commit(&commit)?;
    let analysis = run_analysis(&candidate.lifecore)?;
    let view = state::latest_view(&candidate.lifecore);
    let mock_indices = candidate
        .lifecore
        .window(view.items())
        .filter_map(|entry| {
            candidate
                .lifecore
                .get_with_circumstance(entry.entry_context())
                .and_then(|(_, circumstance)| {
                    matches!(circumstance, lifecore::Circumstance::Mock)
                        .then_some(entry.entry_context().entry().0)
                })
        })
        .collect::<Vec<_>>();
    let entries = mock_indices
        .iter()
        .map(|index| transaction_entry(&candidate, *index, view))
        .collect::<Result<Vec<_>>>()?;
    let universe = messages::universe(analysis.0.clone());
    let records = messages::records(
        &universe,
        &candidate.cli.dismissals,
        &candidate.lifecore,
        view,
    );
    if analysis.has_error() {
        render_transaction(&candidate, view, "aborted", &entries, &records, format)?;
        bail!("candidate analysis contains an Inconsistency");
    }
    if !commit.commit {
        render_transaction(&candidate, view, "dry_run", &entries, &records, format)?;
        return Ok(());
    }
    if let Some(expected) = commit.expect_head.as_deref() {
        verify_head(path, expected)?;
    }
    candidate.lifecore.apply()?;
    if let Some(digest) = dismiss_on_commit
        && !messages::is_dismissed(&candidate.cli.dismissals, &digest)
    {
        candidate.cli.dismissals.push(Dismissal {
            version: MESSAGE_ID_VERSION,
            digest,
        });
        candidate
            .cli
            .dismissals
            .sort_by(|left, right| left.digest.cmp(&right.digest));
    }
    candidate.save(path)?;
    let committed_view = state::latest_view(&candidate.lifecore);
    let committed_entries = mock_indices
        .iter()
        .map(|index| transaction_entry(&candidate, *index, committed_view))
        .collect::<Result<Vec<_>>>()?;
    render_transaction(
        &candidate,
        committed_view,
        "committed",
        &committed_entries,
        &records,
        format,
    )
}

fn validate_commit(commit: &CommitArgs) -> Result<()> {
    if commit.expect_head.is_some() && !commit.commit {
        bail!("--expect-head requires --commit");
    }
    Ok(())
}

fn verify_head(path: &Path, expected: &str) -> Result<()> {
    let current = StateFile::load(path)?;
    let Some(actual) = current.lifecore.last() else {
        if expected == "EMPTY" {
            return Ok(());
        }
        bail!("expected canonical head {expected}, actual head is EMPTY");
    };
    if expected == "EMPTY" {
        let durable =
            locator::durable(current.cli.state_uuid, &current.lifecore, actual.entry().0)?;
        bail!("expected canonical head EMPTY, actual head is {durable}");
    }
    if expected == "HEAD" {
        bail!("HEAD is not accepted by --expect-head");
    }
    let supplied = Locator::from_str(expected)?;
    let (_, durable) = locator::resolve(
        current.cli.state_uuid,
        &current.lifecore,
        &supplied,
        state::latest_view(&current.lifecore),
    )?;
    if durable.index != actual.entry().0 {
        let actual = locator::durable(current.cli.state_uuid, &current.lifecore, actual.entry().0)?;
        bail!("expected canonical head {durable}, actual head is {actual}");
    }
    Ok(())
}

fn graphviz(prepared: &Prepared, args: GraphvizArgs) -> Result<()> {
    let view = view_from_at(prepared, args.at.as_ref())?;
    let nodes = if args.full {
        prepared.state.lifecore.nodes(view).collect::<Vec<_>>()
    } else {
        prepared
            .state
            .lifecore
            .fresh_nodes(view)
            .collect::<Vec<_>>()
    };
    let edges = if args.full {
        prepared.state.lifecore.edges(view).collect::<Vec<_>>()
    } else {
        prepared
            .state
            .lifecore
            .fresh_edges(view)
            .collect::<Vec<_>>()
    };
    let mut dot =
        String::from("digraph LifeCore {\n  graph [compound=true];\n  node [shape=mrecord];\n");
    if args.full {
        for (family_index, family) in prepared
            .state
            .lifecore
            .families(view)
            .into_iter()
            .enumerate()
        {
            dot.push_str(&format!(
                "  subgraph cluster_{family_index} {{\n    label=\"family {family_index}\";\n"
            ));
            for member in family.iter() {
                dot.push_str(&format!("    n{};\n", member.entry().0));
            }
            dot.push_str("  }\n");
        }
    }
    for node in &nodes {
        let durable = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            node.e.entry().0,
        )?;
        let aliases = prepared
            .aliases
            .iter()
            .filter(|(_, target)| target.index == node.e.entry().0)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut parts = vec![
            escape_dot(&durable.to_string()),
            escape_dot(&node.ty_name),
            escape_dot(&aliases),
        ];
        if args.fields {
            parts.extend(node.field.iter().map(|(name, value)| {
                escape_dot(&format!(
                    "{name}={}",
                    serde_json::to_string(value).unwrap_or_default()
                ))
            }));
        }
        dot.push_str(&format!(
            "  n{} [label=\"{{{}}}\"];\n",
            node.e.entry().0,
            parts.join("|")
        ));
    }
    for edge in edges {
        let durable = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            edge.e.entry().0,
        )?;
        dot.push_str(&format!(
            "  n{} -> n{} [label=\"{}\\n{}\"];\n",
            edge.from.0,
            edge.to.0,
            escape_dot(&edge.ty_name),
            durable
        ));
    }
    if args.full {
        for node in &nodes {
            if let Some(parent) = node.parent {
                dot.push_str(&format!(
                    "  n{} -> n{} [style=dashed, arrowhead=empty, constraint=false, label=\"parent\"];\n",
                    parent.0,
                    node.e.entry().0
                ));
            }
        }
    }
    dot.push_str("}\n");
    if args.dot {
        fs::write(&args.target, dot)
            .with_context(|| format!("failed to write {}", args.target.display()))?;
        return Ok(());
    }
    let mut child = ProcessCommand::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to launch GraphViz executable `dot`")?;
    child
        .stdin
        .take()
        .context("failed to open GraphViz stdin")?
        .write_all(dot.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "GraphViz failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    fs::write(&args.target, output.stdout)?;
    Ok(())
}

fn escape_dot(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('|', "\\|")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace('\n', "\\n")
}

fn entry_value(
    prepared: &Prepared,
    entry: Entry,
    durable: DurableLocator,
    view: Context,
) -> Result<Value> {
    let context = locator::context_in_view(&prepared.state.lifecore, durable.index, view)
        .ok_or_else(|| anyhow!("entry is outside the selected view"))?;
    let circumstance = prepared
        .state
        .lifecore
        .get_with_circumstance(context)
        .map(|(_, circumstance)| circumstance);
    let aliases = prepared
        .aliases
        .iter()
        .filter(|(_, target)| target.index == durable.index)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let common = json!({
        "index": durable.index,
        "locator": durable.to_string(),
        "full_locator": format!("{}:{}", durable.index, durable.digest),
        "aliases": aliases,
        "fresh": prepared.state.lifecore.is_stale(context) == Some(false),
        "circumstance": circumstance,
    });
    let mut object = common.as_object().unwrap().clone();
    match entry {
    Entry::Node(node) => {
        object.insert("kind".into(), json!("node"));
        object.insert("type".into(), json!(node.ty_name));
        object.insert("body".into(), serde_json::to_value(node.field)?);
        object.insert(
            "parent".into(),
            match node.parent {
            Some(parent) => {
                let locator = locator::durable(
                    prepared.state.cli.state_uuid,
                    &prepared.state.lifecore,
                    parent.0,
                )?;
                json!(locator.to_string())
            }
            None => Value::Null,
            },
        );
    }
    Entry::Edge(edge) => {
        object.insert("kind".into(), json!("edge"));
        object.insert("type".into(), json!(edge.ty_name));
        let from = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            edge.from.0,
        )?;
        let to = locator::durable(
            prepared.state.cli.state_uuid,
            &prepared.state.lifecore,
            edge.to.0,
        )?;
        object.insert("from".into(), json!(from.to_string()));
        object.insert("to".into(), json!(to.to_string()));
    }
    }
    Ok(Value::Object(object))
}

fn transaction_entry(state: &StateFile, index: usize, view: Context) -> Result<Value> {
    let context = locator::context_in_view(&state.lifecore, index, view)
        .ok_or_else(|| anyhow!("transaction entry is unavailable"))?;
    let entry = state.lifecore.get(context).unwrap();
    let prepared = Prepared {
        state: state.clone(),
        aliases: state.cli.aliases.clone(),
        temporary_aliases: BTreeSet::new(),
        added: Vec::new(),
        analysis: Output(Vec::new()),
        inconsistent: false,
    };
    let durable = locator::durable(state.cli.state_uuid, &state.lifecore, index)?;
    entry_value(&prepared, entry, durable, view)
}

fn type_value(declaration: &Type) -> Value {
    json!({
        "name": declaration.name,
        "fields": declaration.body.iter().map(|field| {
            format!("{} = {} : {}", field.name(), field.value_type(), field.status())
        }).collect::<Vec<_>>(),
        "outgoing": declaration.outgoing.iter().collect::<Vec<_>>(),
        "incoming": declaration.incoming.iter().collect::<Vec<_>>(),
        "allows_override": declaration.additional_override.iter().collect::<Vec<_>>(),
    })
}

fn view_value(state: &StateFile, view: Context) -> Result<Value> {
    let Some(last) = state.lifecore.window(view.items()).last() else {
        return Ok(json!({"end": "EMPTY"}));
    };
    let durable = locator::durable(
        state.cli.state_uuid,
        &state.lifecore,
        last.entry_context().entry().0,
    )?;
    Ok(json!({
        "through": durable.to_string(),
        "full_locator": format!("{}:{}", durable.index, durable.digest),
    }))
}

fn render_items<T: Serialize>(
    view: Option<Value>,
    items: &[T],
    format: OutputFormat,
) -> Result<()> {
    let value = json!({"view": view, "items": items});
    match resolved_format(format) {
    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
    OutputFormat::Lines | OutputFormat::Table => {
        for item in value["items"].as_array().unwrap_or(&Vec::new()) {
            println!("{}", serde_json::to_string(item)?);
        }
        if value["items"].as_array().is_some_and(Vec::is_empty) {
            println!("No items.");
        }
    }
    OutputFormat::Auto => unreachable!(),
    }
    Ok(())
}

fn render_transaction<T: Serialize, U: Serialize>(
    state: &StateFile,
    view: Context,
    transaction: &str,
    entries: &[T],
    messages: &[U],
    format: OutputFormat,
) -> Result<()> {
    let value = json!({
        "view": view_value(state, view)?,
        "transaction": transaction,
        "entries": entries,
        "messages": messages,
    });
    match resolved_format(format) {
    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
    OutputFormat::Lines | OutputFormat::Table => {
        println!("transaction: {transaction}");
        for entry in value["entries"].as_array().unwrap_or(&Vec::new()) {
            println!("entry\t{}", serde_json::to_string(entry)?);
        }
        for message in value["messages"].as_array().unwrap_or(&Vec::new()) {
            println!("message\t{}", serde_json::to_string(message)?);
        }
    }
    OutputFormat::Auto => unreachable!(),
    }
    Ok(())
}

fn resolved_format(format: OutputFormat) -> OutputFormat {
    match format {
    OutputFormat::Auto if io::stdout().is_terminal() => OutputFormat::Table,
    OutputFormat::Auto => OutputFormat::Lines,
    value => value,
    }
}

fn interactive(path: &Path, format: OutputFormat) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive mode requires a TTY on standard input and output");
    }
    loop {
        let prepared = prepare(StateFile::load(path)?)?;
        if prepared.inconsistent {
            bail!("attached sidecar is inconsistent; use batch check or sidecar detach");
        }
        let read_only = prepared.state.cli.sidecar.is_some();
        println!("1) Browse nodes");
        println!("2) Query nodes");
        println!("3) Inspect entry");
        println!("4) Messages");
        if !read_only {
            println!("5) Bind alias");
            println!("6) Create Type");
            println!("7) Override Type");
            println!("8) Dismiss message");
            println!("9) Restore message");
            println!("10) Apply suggestion");
        }
        println!("0) Quit");
        match prompt("choice")?.as_str() {
        "1" => {
            let view = state::latest_view(&prepared.state.lifecore);
            let items = prepared
                .state
                .lifecore
                .nodes(view)
                .map(|node| {
                    let durable = locator::durable(
                        prepared.state.cli.state_uuid,
                        &prepared.state.lifecore,
                        node.e.entry().0,
                    )?;
                    entry_value(&prepared, Entry::Node(node), durable, view)
                })
                .collect::<Result<Vec<_>>>()?;
            render_items(Some(view_value(&prepared.state, view)?), &items, format)?;
        }
        "2" => {
            let combinator = match prompt("combinator (all/any/none)")?.as_str() {
            "all" => QueryCombinator::All,
            "any" => QueryCombinator::Any,
            "none" => QueryCombinator::None,
            _ => {
                eprintln!("invalid combinator");
                continue;
            }
            };
            let mut predicates = Vec::new();
            loop {
                let value = prompt("predicate (blank to run)")?;
                if value.is_empty() {
                    break;
                }
                predicates.push(value);
            }
            query_command(
                path,
                prepared,
                QueryArgs {
                    command: QueryCommand::Nodes(QueryNodesArgs {
                        combinator,
                        predicates,
                        at: None,
                        select: None,
                        explain: None,
                        action: Vec::new(),
                    }),
                },
                format,
            )?;
        }
        "3" => {
            let locator = Locator::from_str(&prompt("entry locator")?)?;
            entry_command(
                &prepared,
                EntryArgs {
                    command: EntryCommand::Show { locator },
                },
                format,
            )?;
        }
        "4" => check(
            &prepared,
            CheckArgs {
                include_dismissed: false,
                only_dismissed: false,
                include_resolved: false,
                dismissal_ids: false,
            },
            format,
        )?,
        "5" if !read_only => {
            let name = prompt("alias name")?;
            let locator = Locator::from_str(&prompt("node locator")?)?;
            alias_command(
                path,
                prepared,
                AliasArgs {
                    command: AliasCommand::Set { name, locator },
                },
                format,
            )?;
        }
        "6" if !read_only => {
            let name = prompt("type name")?;
            let fields = prompt_repeated("field declaration")?;
            let head = current_head_guard(&prepared.state)?;
            let args = TypeCreateArgs {
                name,
                fields,
                outgoing: prompt_repeated("outgoing permission")?,
                incoming: prompt_repeated("incoming permission")?,
                allows_override: prompt_repeated("override permission")?,
                commit: CommitArgs {
                    commit: false,
                    expect_head: None,
                },
            };
            type_command(
                path,
                prepared.clone(),
                TypeArgs {
                    command: TypeCommand::Create(args.clone()),
                },
                format,
            )?;
            if prompt("type commit to commit, blank to cancel")? == "commit" {
                let mut args = args;
                args.commit = CommitArgs {
                    commit: true,
                    expect_head: Some(head),
                };
                type_command(
                    path,
                    prepared,
                    TypeArgs {
                        command: TypeCommand::Create(args),
                    },
                    format,
                )?;
            }
        }
        "7" if !read_only => {
            let name = prompt("active type name")?;
            let fields = prompt_repeated("replacement/additional field")?;
            let drops = prompt_repeated("field to drop")?;
            let head = current_head_guard(&prepared.state)?;
            let make = |commit| TypeOverrideArgs {
                name: name.clone(),
                at: None,
                new_name: None,
                fields: fields.clone(),
                drop_field: drops.clone(),
                allow_outgoing: Vec::new(),
                disallow_outgoing: Vec::new(),
                allow_incoming: Vec::new(),
                disallow_incoming: Vec::new(),
                allows_override: Vec::new(),
                disallow_override: Vec::new(),
                commit,
            };
            type_command(
                path,
                prepared.clone(),
                TypeArgs {
                    command: TypeCommand::Override(make(CommitArgs {
                        commit: false,
                        expect_head: None,
                    })),
                },
                format,
            )?;
            if prompt("type commit to commit, blank to cancel")? == "commit" {
                type_command(
                    path,
                    prepared,
                    TypeArgs {
                        command: TypeCommand::Override(make(CommitArgs {
                            commit: true,
                            expect_head: Some(head),
                        })),
                    },
                    format,
                )?;
            }
        }
        "8" if !read_only => message_command(
            path,
            prepared,
            MessageArgs {
                command: MessageCommand::Dismiss {
                    id: prompt("message ID")?,
                },
            },
            format,
        )?,
        "9" if !read_only => message_command(
            path,
            prepared,
            MessageArgs {
                command: MessageCommand::Restore {
                    id: prompt("message ID")?,
                },
            },
            format,
        )?,
        "10" if !read_only => {
            let id = prompt("suggestion message ID")?;
            let head = current_head_guard(&prepared.state)?;
            message_command(
                path,
                prepared.clone(),
                MessageArgs {
                    command: MessageCommand::Apply {
                        id: id.clone(),
                        keep_message: false,
                        commit: CommitArgs {
                            commit: false,
                            expect_head: None,
                        },
                    },
                },
                format,
            )?;
            if prompt("type commit to commit, blank to cancel")? == "commit" {
                message_command(
                    path,
                    prepared,
                    MessageArgs {
                        command: MessageCommand::Apply {
                            id,
                            keep_message: false,
                            commit: CommitArgs {
                                commit: true,
                                expect_head: Some(head),
                            },
                        },
                    },
                    format,
                )?;
            }
        }
        "0" => return Ok(()),
        _ => eprintln!("invalid choice"),
        }
    }
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}

fn prompt_repeated(label: &str) -> Result<Vec<String>> {
    let mut values = Vec::new();
    loop {
        let value = prompt(&format!("{label} (blank when done)"))?;
        if value.is_empty() {
            return Ok(values);
        }
        values.push(value);
    }
}

fn current_head_guard(state: &StateFile) -> Result<String> {
    state.lifecore.last().map_or_else(
        || Ok("EMPTY".to_string()),
        |last| {
            Ok(
                locator::durable(state.cli.state_uuid, &state.lifecore, last.entry().0)?
                    .to_string(),
            )
        },
    )
}
