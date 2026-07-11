mod graphviz;

use std::{
    collections::BTreeMap,
    env, fs,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use graphviz::{GraphVizConfig, GraphVizConfigParentMode};
use itertools::Itertools;
use lifecore::{
    AnalysisOutput, AnalysisReport, AnalysisRunner, Analyzer, AnalyzerFactory,
    ConstraintSystemMessage, EdgeExt, EventContext, LifeCore, LifeCoreError, LifeCoreList,
    NodeBodyExt, NodeTypeClass, PayloadExt, SuggestedAction,
};
use lifecore_typecheck::TypeCheckFactory;
use regex::Regex;
use serde::{Deserialize, Serialize};

const DEFAULT_DB_FILE: &str = "LifeCore.json";

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(long, global = true)]
    ascii: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        force: bool,
    },
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    Connect {
        from: String,
        edge_type: String,
        to: String,
    },
    Types,
    Find(FindArgs),
    Names,
    Name {
        codename: String,
        target: String,
    },
    Suggest {
        #[command(subcommand)]
        command: SuggestCommand,
    },
    Graphviz(GraphVizArgs),
}

#[derive(Subcommand)]
enum NodeCommand {
    Add(AddNodeArgs),
    Show { target: String },
}

#[derive(Args)]
struct AddNodeArgs {
    codename: Option<String>,
    #[arg(long = "type")]
    ty: String,
    #[arg(long)]
    payload: String,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    unnamed: bool,
}

#[derive(Args)]
struct FindArgs {
    #[arg(long = "type")]
    ty: Option<String>,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long)]
    any: Option<String>,
    #[arg(long)]
    name_index: Option<usize>,
    #[arg(long)]
    codename: Option<String>,
}

#[derive(Args)]
struct GraphVizArgs {
    destination_file: PathBuf,
    #[arg(long, value_enum, default_value_t = GraphVizFamilyArg::NoFamily)]
    family: GraphVizFamilyArg,
    #[arg(long = "type")]
    subset_type: Vec<String>,
    #[arg(long = "node-type")]
    subset_node_type: Vec<String>,
    #[arg(long = "edge-type")]
    subset_edge_type: Vec<String>,
    #[arg(long)]
    no_colorize: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum GraphVizFamilyArg {
    AllFamilyIfAnyMember,
    AllFamilyIfAnyFresh,
    AllFamilyIfAnyParent,
    OnlyFresh,
    OnlyParents,
    NoFamily,
}

#[derive(Subcommand)]
enum SuggestCommand {
    All {
        #[arg(long)]
        apply: Option<usize>,
    },
    Edges {
        #[arg(long)]
        apply: Option<usize>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct CliState {
    version: u32,
    db: LifeCoreList,
    codenames: BTreeMap<String, EventContext>,
}

impl Default for CliState {
    fn default() -> Self {
        Self {
            version: 1,
            db: LifeCoreList::new(),
            codenames: BTreeMap::new(),
        }
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    execute(cli)
}

fn execute(cli: Cli) -> Result<()> {
    let path = db_path()?;
    if let Command::Init { force } = cli.command {
        return init_db(&path, force, cli.ascii);
    }

    let mut state = load_state(&path)?;
    ensure_default_analyzers(&mut state.db);

    match cli.command {
    Command::Init { .. } => unreachable!("init is handled before loading state"),
    Command::Node { command } => match command {
    NodeCommand::Add(args) => add_node(&mut state, &path, args, cli.ascii),
    NodeCommand::Show { target } => show_node(&state, &target, cli.ascii),
    },
    Command::Connect {
        from,
        edge_type,
        to,
    } => connect_nodes(&mut state, &path, &from, &edge_type, &to, cli.ascii),
    Command::Types => list_types(&state, cli.ascii),
    Command::Find(args) => find_nodes(&mut state, &path, args, cli.ascii),
    Command::Names => list_names(&state, cli.ascii),
    Command::Name { codename, target } => name_node(&mut state, &path, &codename, &target),
    Command::Suggest { command } => match command {
    SuggestCommand::All { apply } => {
        suggest(&mut state, &path, apply, cli.ascii, SuggestionFilter::All)
    }
    SuggestCommand::Edges { apply } => {
        suggest(&mut state, &path, apply, cli.ascii, SuggestionFilter::Edges)
    }
    },
    Command::Graphviz(args) => render_graphviz(&state, args),
    }
}

fn render_graphviz(state: &CliState, args: GraphVizArgs) -> Result<()> {
    graphviz::graphviz(
        &state.db,
        GraphVizConfig {
            destination_file: args.destination_file,
            family: args.family.into(),
            subset_type: args.subset_type,
            subset_node_type: args.subset_node_type,
            subset_edge_type: args.subset_edge_type,
            colorize: !args.no_colorize,
        },
    )
}

impl From<GraphVizFamilyArg> for GraphVizConfigParentMode {
    fn from(value: GraphVizFamilyArg) -> Self {
        match value {
        GraphVizFamilyArg::AllFamilyIfAnyMember => Self::AllFamilyIfAnyMember,
        GraphVizFamilyArg::AllFamilyIfAnyFresh => Self::AllFamilyIfAnyFresh,
        GraphVizFamilyArg::AllFamilyIfAnyParent => Self::AllFamilyIfAnyParent,
        GraphVizFamilyArg::OnlyFresh => Self::OnlyFresh,
        GraphVizFamilyArg::OnlyParents => Self::OnlyParents,
        GraphVizFamilyArg::NoFamily => Self::NoFamily,
        }
    }
}

fn db_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("LIFECORE_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    Ok(env::current_dir()
        .context("could not determine current directory")?
        .join(DEFAULT_DB_FILE))
}

fn load_state(path: &Path) -> Result<CliState> {
    match fs::read_to_string(path) {
    Ok(raw) => {
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }
    Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn init_db(path: &Path, force: bool, ascii: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite it",
            path.display()
        );
    }
    let mut state = CliState::default();
    ensure_default_analyzers(&mut state.db);
    let output = run_full_analysis(&state.db)?;
    if !output.0.is_empty() {
        print_reports(&output, ascii);
    }
    if output.has_error() {
        bail!("initial database rejected because analysis found errors");
    }
    save_state(path, &state)
}

fn save_state(path: &Path, state: &CliState) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or(DEFAULT_DB_FILE);
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn ensure_default_analyzers(db: &mut LifeCoreList) {
    let existing = db.current_analysis_suite().0.into_iter().collect_vec();
    let typecheck_name = TypeCheckFactory::<LifeCoreList>::default().analyzer_name();
    let interactive_name = InteractiveFactory::<LifeCoreList>::default().analyzer_name();
    if !existing
        .iter()
        .any(|x| x.name == typecheck_name && x.config.is_empty())
    {
        db.add_analyzer(typecheck_name, "");
    }
    if !existing
        .iter()
        .any(|x| x.name == interactive_name && x.config.is_empty())
    {
        db.add_analyzer(interactive_name, "");
    }
}

fn add_node(state: &mut CliState, path: &Path, args: AddNodeArgs, ascii: bool) -> Result<()> {
    if args.unnamed && args.codename.is_some() {
        bail!("node add accepts either a codename or --unnamed, not both");
    }
    if !args.unnamed && args.codename.is_none() {
        bail!("node add requires a codename unless --unnamed is passed");
    }

    let payload = parse_payload(&args.payload)?;
    let parent = args
        .parent
        .as_deref()
        .map(|target| resolve_node_ref(state, target))
        .transpose()?;
    let added = state.db.add(&args.ty, payload, parent)?;
    if let Some(codename) = args.codename {
        state.codenames.insert(codename, added.e);
    }
    commit_db_mutation(state, path, ascii)
}

fn connect_nodes(
    state: &mut CliState,
    path: &Path,
    from: &str,
    edge_type: &str,
    to: &str,
    ascii: bool,
) -> Result<()> {
    let from = resolve_node_ref(state, from)?;
    let to = resolve_node_ref(state, to)?;
    state.db.connect(from, edge_type, to)?;
    commit_db_mutation(state, path, ascii)
}

fn name_node(state: &mut CliState, path: &Path, codename: &str, target: &str) -> Result<()> {
    let target = resolve_node_ref(state, target)?;
    state.codenames.insert(codename.to_string(), target);
    save_state(path, state)
}

fn list_names(state: &CliState, ascii: bool) -> Result<()> {
    let rows = state
        .codenames
        .iter()
        .map(|(name, ctx)| vec![name.clone(), ctx.to_string()])
        .collect_vec();
    println!("{}", render_table(&["codename", "event"], &rows, ascii));
    Ok(())
}

fn list_types(state: &CliState, ascii: bool) -> Result<()> {
    let rows = state
        .db
        .types(EventContext::universe())
        .sorted_by(|left, right| left.0.cmp(&right.0))
        .map(|(name, (ctx, ty))| {
            vec![
                name,
                ctx.to_string(),
                node_type_class_text(&ty.class).to_string(),
                ty.field.len().to_string(),
                ty.outgoing.join(","),
                ty.incoming.join(","),
            ]
        })
        .collect_vec();
    println!(
        "{}",
        render_table(
            &["type", "event", "class", "fields", "outgoing", "incoming"],
            &rows,
            ascii,
        )
    );
    Ok(())
}

fn show_node(state: &CliState, target: &str, ascii: bool) -> Result<()> {
    let ctx = resolve_node_ref(state, target)?;
    let node = state.db.node(ctx);
    let details = vec![
        vec!["event".to_string(), node.e.to_string()],
        vec!["type".to_string(), node.ty_name],
        vec![
            "parent".to_string(),
            node.parent
                .map(|x| x.to_string())
                .unwrap_or_else(|| "".to_string()),
        ],
    ];
    println!("{}", render_table(&["key", "value"], &details, ascii));

    let rows = node
        .field
        .0
        .iter()
        .sorted_by(|left, right| left.0.cmp(right.0))
        .map(|(name, payload)| vec![name.clone(), payload_kind(payload), payload_text(payload)])
        .collect_vec();
    println!(
        "{}",
        render_table(&["field", "kind", "value"], &rows, ascii)
    );
    Ok(())
}

fn find_nodes(state: &mut CliState, path: &Path, args: FindArgs, ascii: bool) -> Result<()> {
    if args.name_index.is_some() != args.codename.is_some() {
        bail!("--name-index and --codename must be passed together");
    }

    let field_patterns = args
        .fields
        .iter()
        .map(|field| {
            let (name, pattern) = field
                .split_once('=')
                .ok_or_else(|| anyhow!("field filter must use name=regex"))?;
            Ok((name.to_string(), Regex::new(pattern)?))
        })
        .collect::<Result<Vec<_>>>()?;
    let any_pattern = args
        .any
        .as_deref()
        .map(Regex::new)
        .transpose()
        .context("invalid --any regex")?;

    let matches = matching_nodes(
        state,
        args.ty.as_deref(),
        &field_patterns,
        any_pattern.as_ref(),
    );
    if let (Some(index), Some(codename)) = (args.name_index, args.codename) {
        let target = matches
            .get(index)
            .ok_or_else(|| anyhow!("find result index {index} is out of range"))?
            .0;
        state.codenames.insert(codename, target);
        save_state(path, state)?;
    }

    let rows = matches
        .iter()
        .enumerate()
        .map(|(index, (ctx, ty, summary))| {
            vec![
                index.to_string(),
                ctx.to_string(),
                ty.clone(),
                summary.clone(),
            ]
        })
        .collect_vec();
    println!(
        "{}",
        render_table(&["index", "event", "type", "summary"], &rows, ascii)
    );
    Ok(())
}

fn matching_nodes(
    state: &CliState,
    ty: Option<&str>,
    field_patterns: &[(String, Regex)],
    any_pattern: Option<&Regex>,
) -> Vec<(EventContext, String, String)> {
    state
        .db
        .nodes(EventContext::universe())
        .filter(|(ctx, node)| {
            !state.db.is_stale(*ctx)
                && ty.is_none_or(|ty| node.ty_name == ty)
                && field_patterns.iter().all(|(field, pattern)| {
                    node.field
                        .0
                        .get(field)
                        .is_some_and(|payload| pattern.is_match(&payload_search_text(payload)))
                })
                && any_pattern.is_none_or(|pattern| {
                    node.field
                        .0
                        .values()
                        .any(|payload| pattern.is_match(&payload_search_text(payload)))
                })
        })
        .map(|(ctx, node)| {
            let summary = node
                .field
                .0
                .iter()
                .sorted_by(|left, right| left.0.cmp(right.0))
                .take(3)
                .map(|(name, payload)| format!("{name}={}", payload_search_text(payload)))
                .join("; ");
            (ctx, node.ty_name, summary)
        })
        .sorted_by(|left, right| left.0.cmp(&right.0))
        .collect_vec()
}

enum SuggestionFilter {
    All,
    Edges,
}

fn suggest(
    state: &mut CliState,
    path: &Path,
    apply: Option<usize>,
    ascii: bool,
    filter: SuggestionFilter,
) -> Result<()> {
    let output = run_full_analysis(&state.db)?;
    if output.has_error() {
        print_reports(&output, ascii);
        bail!("cannot apply suggestions while analysis has errors");
    }
    let suggestions = suggestions_from_output(output, filter);
    if let Some(index) = apply {
        let action = suggestions
            .get(index)
            .ok_or_else(|| anyhow!("suggestion index {index} is out of range"))?
            .1
            .clone();
        apply_suggestion(state, action)?;
        commit_db_mutation(state, path, ascii)?;
    } else {
        print_suggestions(&suggestions, ascii);
    }
    Ok(())
}

fn apply_suggestion(state: &mut CliState, action: SuggestedAction) -> Result<()> {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => {
        state.db.add(&ty_name, payload, parent)?;
    }
    SuggestedAction::AddEdge { from, ty_name, to } => {
        state.db.connect(from, &ty_name, to)?;
    }
    }
    Ok(())
}

fn commit_db_mutation(state: &CliState, path: &Path, ascii: bool) -> Result<()> {
    let output = run_full_analysis(&state.db)?;
    if !output.0.is_empty() {
        print_reports(&output, ascii);
    }
    if output.has_error() {
        bail!("mutation rejected because analysis found errors");
    }
    save_state(path, state)
}

fn run_full_analysis(db: &LifeCoreList) -> Result<AnalysisOutput> {
    let mut reports = db.basic_validation().0;
    let mut runner = AnalysisRunner::<LifeCoreList>::new();
    runner.register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))?;
    runner.register_analyzer_kind(Box::new(InteractiveFactory::<LifeCoreList>::default()))?;
    reports.extend(runner.run(db)?.0);
    Ok(AnalysisOutput(reports))
}

fn suggestions_from_output(
    output: AnalysisOutput,
    filter: SuggestionFilter,
) -> Vec<(ConstraintSystemMessage, SuggestedAction)> {
    output
        .0
        .into_iter()
        .filter_map(|report| match report {
        AnalysisReport::Suggestion(message, action) => Some((message, action)),
        _ => None,
        })
        .filter(|(_, action)| match filter {
        SuggestionFilter::All => true,
        SuggestionFilter::Edges => matches!(action, SuggestedAction::AddEdge { .. }),
        })
        .sorted_by(|left, right| {
            suggestion_sort_key(&left.0, &left.1).cmp(&suggestion_sort_key(&right.0, &right.1))
        })
        .collect_vec()
}

fn suggestion_sort_key(message: &ConstraintSystemMessage, action: &SuggestedAction) -> String {
    match action {
    SuggestedAction::AddNode {
        ty_name, parent, ..
    } => {
        format!(
            "{}|AddNode|{}|{}",
            message.system_name,
            ty_name,
            parent
                .map(|x| x.to_string())
                .unwrap_or_else(|| "".to_string())
        )
    }
    SuggestedAction::AddEdge { from, ty_name, to } => {
        format!(
            "{}|AddEdge|{}|{}|{}",
            message.system_name, ty_name, from, to
        )
    }
    }
}

fn parse_payload(raw: &str) -> Result<NodeBodyExt> {
    serde_json::from_str(raw).context("payload must be a JSON object matching NodeBodyExt serde")
}

fn resolve_node_ref(_state: &CliState, _target: &str) -> Result<EventContext> {
    bail!("To Be Removed")
    // let ctx = if let Some(ctx) = state.codenames.get(target) {
    //     *ctx
    // } else {
    //     let index = target
    //         .parse::<usize>()
    //         .with_context(|| format!("unknown codename or event index {target:?}"))?;
    //     EventContext::universe().look_raw(index)
    // };
    // match state.db.get(ctx) {
    // Some(event) if event.is_node() => Ok(ctx),
    // Some(_) => bail!("{target:?} refers to an edge, not a node"),
    // None => bail!("{target:?} does not refer to an existing event"),
    // }
}

fn payload_kind(payload: &PayloadExt) -> String {
    match payload {
    PayloadExt::PreciseTime(_) => "PreciseTime",
    PayloadExt::Text(_) => "Text",
    PayloadExt::Number(_) => "Number",
    PayloadExt::Serialized(_) => "Serialized",
    }
    .to_string()
}

fn node_type_class_text(class: &NodeTypeClass) -> &'static str {
    match class {
    NodeTypeClass::Spike => "Spike",
    NodeTypeClass::Hub => "Hub",
    NodeTypeClass::Toothpick => "Toothpick",
    NodeTypeClass::EdgeNode => "EdgeNode",
    NodeTypeClass::Regular => "Regular",
    }
}

fn payload_search_text(payload: &PayloadExt) -> String {
    match payload {
    PayloadExt::PreciseTime(timestamp) => timestamp.to_string(),
    PayloadExt::Text(value) => value.clone(),
    PayloadExt::Number(value) => value.to_string(),
    PayloadExt::Serialized(value) => value.to_string(),
    }
}

fn payload_text(payload: &PayloadExt) -> String {
    match payload {
    PayloadExt::Serialized(value) => {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    }
    _ => payload_search_text(payload),
    }
}

fn print_reports(output: &AnalysisOutput, ascii: bool) {
    let rows = output.0.iter().map(report_row).collect_vec();
    println!(
        "{}",
        render_table(
            &["kind", "system", "event", "message", "action"],
            &rows,
            ascii
        )
    );
}

fn report_row(report: &AnalysisReport) -> Vec<String> {
    match report {
    AnalysisReport::BasicValidationFailure {
        offending_event,
        reason,
    } => vec![
        "error".to_string(),
        "BasicValidation".to_string(),
        offending_event.to_string(),
        format!("{reason:?}"),
        "".to_string(),
    ],
    AnalysisReport::BasicValidationWarning {
        offending_event,
        message,
    } => vec![
        "warning".to_string(),
        "BasicValidation".to_string(),
        offending_event.to_string(),
        message.clone(),
        "".to_string(),
    ],
    AnalysisReport::LogInfo {
        offending_event,
        message,
    } => vec![
        "info".to_string(),
        "Log".to_string(),
        offending_event.to_string(),
        message.clone(),
        "".to_string(),
    ],
    AnalysisReport::Inconsistency(message) => vec![
        "error".to_string(),
        message.system_name.clone(),
        message.offending_event.to_string(),
        message.message.clone(),
        "".to_string(),
    ],
    AnalysisReport::Warning(message) => vec![
        "warning".to_string(),
        message.system_name.clone(),
        message.offending_event.to_string(),
        message.message.clone(),
        "".to_string(),
    ],
    AnalysisReport::Suggestion(message, action) => vec![
        "suggestion".to_string(),
        message.system_name.clone(),
        message.offending_event.to_string(),
        message.message.clone(),
        action_text(action),
    ],
    }
}

fn print_suggestions(suggestions: &[(ConstraintSystemMessage, SuggestedAction)], ascii: bool) {
    let rows = suggestions
        .iter()
        .enumerate()
        .map(|(index, (message, action))| {
            vec![
                index.to_string(),
                message.system_name.clone(),
                message.offending_event.to_string(),
                message.message.clone(),
                action_text(action),
            ]
        })
        .collect_vec();
    println!(
        "{}",
        render_table(
            &["index", "system", "event", "message", "action"],
            &rows,
            ascii
        )
    );
}

fn action_text(action: &SuggestedAction) -> String {
    match action {
    SuggestedAction::AddNode {
        ty_name, parent, ..
    } => {
        format!(
            "node add --unnamed --type {}{}",
            ty_name,
            parent
                .map(|x| format!(" --parent {x}"))
                .unwrap_or_else(|| "".to_string())
        )
    }
    SuggestedAction::AddEdge { from, ty_name, to } => {
        format!("connect {from} {ty_name} {to}")
    }
    }
}

fn render_table(headers: &[&str], rows: &[Vec<String>], ascii: bool) -> String {
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain([header.chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect_vec();
    let chars = if ascii {
        TableChars {
            top_left: '+',
            top_mid: '+',
            top_right: '+',
            mid_left: '+',
            mid_mid: '+',
            mid_right: '+',
            bottom_left: '+',
            bottom_mid: '+',
            bottom_right: '+',
            horizontal: '-',
            vertical: '|',
        }
    } else {
        TableChars {
            top_left: '┌',
            top_mid: '┬',
            top_right: '┐',
            mid_left: '├',
            mid_mid: '┼',
            mid_right: '┤',
            bottom_left: '└',
            bottom_mid: '┴',
            bottom_right: '┘',
            horizontal: '─',
            vertical: '│',
        }
    };

    let mut out = String::new();
    out.push_str(&border(
        &widths,
        chars.top_left,
        chars.top_mid,
        chars.top_right,
        chars.horizontal,
    ));
    out.push('\n');
    out.push_str(&row(
        &headers.iter().map(|x| x.to_string()).collect_vec(),
        &widths,
        chars.vertical,
    ));
    out.push('\n');
    out.push_str(&border(
        &widths,
        chars.mid_left,
        chars.mid_mid,
        chars.mid_right,
        chars.horizontal,
    ));
    for row_cells in rows {
        out.push('\n');
        out.push_str(&row(row_cells, &widths, chars.vertical));
    }
    out.push('\n');
    out.push_str(&border(
        &widths,
        chars.bottom_left,
        chars.bottom_mid,
        chars.bottom_right,
        chars.horizontal,
    ));
    out
}

struct TableChars {
    top_left: char,
    top_mid: char,
    top_right: char,
    mid_left: char,
    mid_mid: char,
    mid_right: char,
    bottom_left: char,
    bottom_mid: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

fn border(widths: &[usize], left: char, mid: char, right: char, horizontal: char) -> String {
    let mut out = String::new();
    out.push(left);
    for (index, width) in widths.iter().enumerate() {
        out.extend(std::iter::repeat_n(horizontal, width + 2));
        out.push(if index + 1 == widths.len() {
            right
        } else {
            mid
        });
    }
    out
}

fn row(cells: &[String], widths: &[usize], vertical: char) -> String {
    let mut out = String::new();
    out.push(vertical);
    for (index, width) in widths.iter().enumerate() {
        let cell = cells.get(index).map_or("", String::as_str);
        out.push(' ');
        out.push_str(cell);
        out.extend(std::iter::repeat_n(
            ' ',
            width.saturating_sub(cell.chars().count()) + 1,
        ));
        out.push(vertical);
    }
    out
}

pub struct InteractiveFactory<LC: LifeCore> {
    _marker: PhantomData<LC>,
}

impl<LC: LifeCore> Default for InteractiveFactory<LC> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<LC: LifeCore + 'static> AnalyzerFactory<LC> for InteractiveFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> std::result::Result<Box<dyn Analyzer<LC>>, LifeCoreError> {
        Ok(Box::new(InteractiveAnalyzer {
            config: config.to_string(),
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        "Interactive"
    }
}

pub struct InteractiveAnalyzer {
    config: String,
}

impl<LC: LifeCore + 'static> Analyzer<LC> for InteractiveAnalyzer {
    fn initialize(&mut self, _lc: &LC) -> AnalysisOutput {
        AnalysisOutput(vec![])
    }

    fn analyze(&self, lc: &LC) -> AnalysisOutput {
        if lc.total_event_count() == 0 {
            return AnalysisOutput(vec![]);
        }
        let latest = lc.last();
        let Some(latest_event) = lc.get(latest) else {
            return AnalysisOutput(vec![]);
        };
        if !latest_event.is_node() {
            return AnalysisOutput(vec![]);
        }
        let new_node = latest_event.as_node();
        let Some(old_node_ctx) = new_node.parent else {
            return AnalysisOutput(vec![]);
        };

        let before_new = latest.exclusive_left_context();
        let current = latest.inclusive_left_context();
        let system_name = InteractiveFactory::<LC>::default()
            .analyzer_name()
            .to_string();
        let mut reports = vec![];
        for (_, edge) in lc.edges(before_new).filter(|(_, edge)| {
            edge.from.absolute() == old_node_ctx.absolute()
                || edge.to.absolute() == old_node_ctx.absolute()
        }) {
            let (from, to, other) = if edge.from.absolute() == old_node_ctx.absolute() {
                (
                    current.look_at(new_node.e),
                    current.look_at(edge.to),
                    current.look_at(edge.to),
                )
            } else {
                (
                    current.look_at(edge.from),
                    current.look_at(new_node.e),
                    current.look_at(edge.from),
                )
            };

            if lc.is_stale(other)
                || !edge_is_type_allowed(lc, current, from, &edge.ty_name, to)
                || edge_exists(lc, current, from, &edge.ty_name, to)
                || sibling_has_similar_edge(lc, current, new_node.e, other, &edge.ty_name, &edge)
            {
                continue;
            }

            let action = SuggestedAction::AddEdge {
                from,
                ty_name: edge.ty_name.clone(),
                to,
            };
            reports.push(AnalysisReport::Suggestion(
                ConstraintSystemMessage {
                    offending_event: latest,
                    system_name: system_name.clone(),
                    system_config: self.config.clone(),
                    relevance: vec![latest, old_node_ctx],
                    message: format!(
                        "Node {} superseded {}; consider preserving edge {}",
                        latest, old_node_ctx, edge.ty_name
                    ),
                },
                action,
            ));
        }
        reports.sort_by(|left, right| report_sort_key(left).cmp(&report_sort_key(right)));
        AnalysisOutput(reports)
    }
}

fn edge_is_type_allowed<LC: LifeCore>(
    lc: &LC,
    ctx: EventContext,
    from: EventContext,
    ty_name: &str,
    to: EventContext,
) -> bool {
    let from_node = lc.node(ctx.look_at(from));
    let to_node = lc.node(ctx.look_at(to));
    from_node
        .ty
        .as_ref()
        .is_some_and(|ty| ty.outgoing.iter().any(|allowed| allowed == ty_name))
        && to_node
            .ty
            .as_ref()
            .is_some_and(|ty| ty.incoming.iter().any(|allowed| allowed == ty_name))
}

fn edge_exists<LC: LifeCore>(
    lc: &LC,
    ctx: EventContext,
    from: EventContext,
    ty_name: &str,
    to: EventContext,
) -> bool {
    lc.edges(ctx).any(|(_, edge)| {
        !lc.is_stale(edge.e)
            && edge.from.absolute() == from.absolute()
            && edge.to.absolute() == to.absolute()
            && edge.ty_name == ty_name
    })
}

fn sibling_has_similar_edge<LC: LifeCore>(
    lc: &LC,
    ctx: EventContext,
    new_node: EventContext,
    other: EventContext,
    ty_name: &str,
    old_edge: &EdgeExt,
) -> bool {
    lc.nodes(ctx).any(|(candidate, _)| {
        candidate.absolute() != new_node.absolute()
            && lc.are_siblings(ctx.look_at(candidate), ctx.look_at(new_node))
            && lc.edges(ctx).any(|(_, edge)| {
                !lc.is_stale(edge.e)
                    && edge.ty_name == ty_name
                    && if old_edge.from.absolute() == other.absolute() {
                        edge.from.absolute() == other.absolute()
                            && edge.to.absolute() == candidate.absolute()
                    } else {
                        edge.from.absolute() == candidate.absolute()
                            && edge.to.absolute() == other.absolute()
                    }
            })
    })
}

fn report_sort_key(report: &AnalysisReport) -> String {
    match report {
    AnalysisReport::Suggestion(message, action) => suggestion_sort_key(message, action),
    _ => format!("{report:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_table_uses_ascii_borders() {
        let table = render_table(&["a"], &[vec!["b".to_string()]], true);
        assert!(table.contains("+---+"));
        assert!(table.contains("| a |"));
    }

    #[test]
    fn unicode_table_uses_box_drawing_borders() {
        let table = render_table(&["a"], &[vec!["b".to_string()]], false);
        assert!(table.contains("┌───┐"));
        assert!(table.contains("│ a │"));
    }

    #[test]
    fn payload_serialized_search_text_is_json() {
        let payload = PayloadExt::Serialized(serde_json::json!({"x": 1}));
        assert_eq!(payload_search_text(&payload), r#"{"x":1}"#);
    }

    #[test]
    fn interactive_analyzer_handles_empty_database() {
        let lc = LifeCoreList::new();
        let analyzer = InteractiveAnalyzer {
            config: String::new(),
        };

        let output = analyzer.analyze(&lc);

        assert!(output.0.is_empty());
    }

    #[test]
    fn interactive_analyzer_handles_latest_edge() {
        let mut lc = LifeCoreList::new();
        let left = lc
            .add(
                "",
                NodeBodyExt::from(&[("name", PayloadExt::Text("left".to_string()))]),
                None,
            )
            .unwrap();
        let right = lc
            .add(
                "",
                NodeBodyExt::from(&[("name", PayloadExt::Text("right".to_string()))]),
                None,
            )
            .unwrap();
        lc.connect(left.e, "edge", right.e).unwrap();
        let analyzer = InteractiveAnalyzer {
            config: String::new(),
        };

        let output = analyzer.analyze(&lc);

        assert!(output.0.is_empty());
    }

    #[test]
    fn interactive_analyzer_handles_untyped_supersede_with_old_edges() {
        let mut lc = LifeCoreList::new();
        let old = lc
            .add(
                "",
                NodeBodyExt::from(&[("name", PayloadExt::Text("old".to_string()))]),
                None,
            )
            .unwrap();
        let other = lc
            .add(
                "",
                NodeBodyExt::from(&[("name", PayloadExt::Text("other".to_string()))]),
                None,
            )
            .unwrap();
        lc.connect(old.e, "edge", other.e).unwrap();
        lc.add(
            "",
            NodeBodyExt::from(&[("name", PayloadExt::Text("new".to_string()))]),
            Some(old.e),
        )
        .unwrap();
        let analyzer = InteractiveAnalyzer {
            config: String::new(),
        };

        let output = analyzer.analyze(&lc);

        assert!(output.0.is_empty());
    }
}
