use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{locator::EntryLocator, reference::AliasName};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Auto,
    Table,
    Lines,
    Json,
}

#[derive(Args, Clone, Debug, Default)]
pub struct FormatArgs {
    #[arg(
        long,
        value_enum,
        default_value_t,
        help = "Select terminal or structured output"
    )]
    pub format: OutputFormat,
}

#[derive(Parser, Debug)]
#[command(
    name = "lifecore",
    bin_name = "lifecore",
    version,
    about = "A typed, versioned graph database",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn validate(&self) -> Result<(), clap::Error> {
        if let Some(command) = &self.command {
            command.validate()?;
        }

        Ok(())
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Create a new canonical state file without replacing an existing file")]
    Init(InitArgs),

    #[command(about = "Inspect append-log entries")]
    Entry {
        #[command(subcommand)]
        command: EntryCommand,
    },

    #[command(about = "Stage or commit a node append")]
    Add(AddArgs),

    #[command(about = "Stage or commit an edge append")]
    Connect(ConnectArgs),

    #[command(about = "Evaluate a bounded graph query")]
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },

    #[command(about = "Manage exact, durable node aliases")]
    Alias {
        #[command(subcommand)]
        command: AliasCommand,
    },

    #[command(about = "Inspect and append stored type declarations")]
    Type {
        #[command(subcommand)]
        command: TypeCommand,
    },

    #[command(about = "Manage an attached TOML addendum")]
    Sidecar {
        #[command(subcommand)]
        command: SidecarCommand,
    },

    #[command(about = "Inspect or extend the configured analyzer suite")]
    Analyzer {
        #[command(subcommand)]
        command: AnalyzerCommand,
    },

    #[command(about = "Write a selected graph view as DOT or SVG")]
    Graphviz(GraphvizArgs),

    #[command(about = "Analyze the effective graph and display diagnostics")]
    Check(CheckArgs),

    #[command(about = "Inspect, dismiss, restore, or apply analyzer messages")]
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
}

impl Command {
    pub fn validate(&self) -> Result<(), clap::Error> {
        if let Self::Query { command } = self {
            command.validate()?;
        }

        Ok(())
    }

    pub fn output_format(&self) -> OutputFormat {
        match self {
        Self::Init(args) => args.format.format,
        Self::Entry { command } => command.output_format(),
        Self::Add(args) => args.format.format,
        Self::Connect(args) => args.format.format,
        Self::Query { command } => command.output_format(),
        Self::Alias { command } => command.output_format(),
        Self::Type { command } => command.output_format(),
        Self::Sidecar { command } => command.output_format(),
        Self::Analyzer { command } => command.output_format(),
        Self::Graphviz(_) => OutputFormat::Lines,
        Self::Check(args) => args.format.format,
        Self::Message { command } => command.output_format(),
        }
    }
}

#[derive(Args, Debug)]
pub struct InitArgs {
    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Subcommand, Debug)]
pub enum EntryCommand {
    #[command(about = "Show one node or edge by stable entry locator")]
    Show(EntryShowArgs),
}

impl EntryCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::Show(args) => args.format.format,
        }
    }
}

#[derive(Args, Debug)]
pub struct EntryShowArgs {
    #[arg(help = "Stable node or edge locator")]
    pub locator: String,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct TransactionArgs {
    #[arg(
        long,
        help = "Apply and save the analyzed candidate; otherwise perform a dry run"
    )]
    pub commit: bool,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR|EMPTY",
        requires = "commit",
        help = "Commit only if the canonical head still matches this stable identity"
    )]
    pub expect_head: Option<String>,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    #[arg(help = "Stored node type name")]
    pub r#type: String,

    #[arg(
        long = "field",
        value_name = "FIELD-ASSIGNMENT",
        help = "Add one recursive TOML field assignment"
    )]
    pub fields: Vec<String>,

    #[arg(long, value_name = "NODE-REF", help = "Supersede this parent node")]
    pub parent: Option<String>,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct ConnectArgs {
    #[arg(long = "from", value_name = "NODE-REF", help = "Edge head node")]
    pub from: String,

    #[arg(
        long = "type",
        value_name = "EDGE-TYPE",
        help = "Stored edge type name"
    )]
    pub r#type: String,

    #[arg(long = "to", value_name = "NODE-REF", help = "Edge tail node")]
    pub to: String,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Subcommand, Debug)]
pub enum QueryCommand {
    #[command(about = "Query nodes with all, any, or none predicate combination")]
    Nodes(QueryNodesArgs),
}

impl QueryCommand {
    fn validate(&self) -> Result<(), clap::Error> {
        match self {
        Self::Nodes(args) => {
            args.selection_action()?;

            Ok(())
        }
        }
    }

    fn output_format(&self) -> OutputFormat {
        match self {
        Self::Nodes(args) => args.format.format,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum QueryCombinator {
    All,
    Any,
    None,
}

#[derive(Args, Debug)]
pub struct QueryNodesArgs {
    #[arg(help = "How repeated predicates are combined")]
    pub combinator: QueryCombinator,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        help = "Use the view through and including this entry"
    )]
    pub at: Option<String>,

    #[arg(
        long = "where",
        value_name = "PREDICATE",
        help = "Add one core node-query predicate"
    )]
    pub predicates: Vec<String>,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        conflicts_with = "explain",
        help = "Require this node to belong to the query result"
    )]
    pub select: Option<String>,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        conflicts_with = "select",
        help = "Explain predicate results for this node, including non-members"
    )]
    pub explain: Option<String>,

    #[arg(
        last = true,
        value_name = "ACTION",
        requires = "select",
        help = "Selection action: entry inspect, alias set, type override, or add-child"
    )]
    pub action: Vec<OsString>,

    #[command(flatten)]
    pub format: FormatArgs,
}

impl QueryNodesArgs {
    pub(crate) fn selection_action(&self) -> Result<Option<SelectionAction>, clap::Error> {
        if self.action.is_empty() {
            return Ok(None);
        }

        let args =
            std::iter::once(OsString::from("selection-action")).chain(self.action.iter().cloned());

        SelectionCli::try_parse_from(args).map(|cli| Some(cli.action))
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "selection-action",
    disable_help_flag = true,
    disable_version_flag = true
)]
struct SelectionCli {
    #[command(subcommand)]
    action: SelectionAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum SelectionAction {
    #[command(about = "Inspect the selected entry")]
    Entry {
        #[command(subcommand)]
        command: SelectionEntryCommand,
    },

    #[command(about = "Bind an alias to the selected node")]
    Alias {
        #[command(subcommand)]
        command: SelectionAliasCommand,
    },

    #[command(about = "Override the selected type declaration")]
    Type {
        #[command(subcommand)]
        command: SelectionTypeCommand,
    },

    #[command(about = "Append a child whose parent is the selected node")]
    AddChild(AddChildArgs),
}

#[derive(Subcommand, Debug)]
pub(crate) enum SelectionEntryCommand {
    #[command(about = "Render the selected entry in detail")]
    Inspect,
}

#[derive(Subcommand, Debug)]
pub(crate) enum SelectionAliasCommand {
    #[command(about = "Create or replace an exact alias binding")]
    Set { name: AliasName },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SelectionTypeCommand {
    #[command(about = "Patch and append a child type declaration")]
    Override(SelectedTypeOverrideArgs),
}

#[derive(Args, Debug)]
pub(crate) struct AddChildArgs {
    pub r#type: String,

    #[arg(long = "field", value_name = "FIELD-ASSIGNMENT")]
    pub fields: Vec<String>,

    #[command(flatten)]
    pub transaction: TransactionArgs,
}

#[derive(Subcommand, Debug)]
pub enum AliasCommand {
    #[command(about = "List aliases and target status in a selected view")]
    List(AliasListArgs),

    #[command(about = "Create or replace an exact node binding")]
    Set(AliasSetArgs),

    #[command(about = "Remove one alias binding")]
    Remove(AliasRemoveArgs),
}

impl AliasCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::List(args) => args.format.format,
        Self::Set(args) => args.format.format,
        Self::Remove(args) => args.format.format,
        }
    }
}

#[derive(Args, Debug)]
pub struct AliasListArgs {
    #[arg(long, value_name = "ENTRY-LOCATOR")]
    pub at: Option<String>,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct AliasSetArgs {
    #[arg(help = "Case-sensitive C-style alias name")]
    pub name: AliasName,

    #[arg(help = "Exact node locator to bind")]
    pub locator: EntryLocator,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct AliasRemoveArgs {
    #[arg(help = "Alias name to remove")]
    pub name: AliasName,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Subcommand, Debug)]
pub enum TypeCommand {
    #[command(about = "List current, well-formed stored declarations")]
    List(TypeListArgs),

    #[command(about = "Show a copy-pasteable stored declaration")]
    Show(TypeShowArgs),

    #[command(about = "Stage or commit a new declaration")]
    Create(TypeCreateArgs),

    #[command(about = "Patch a declaration and append its successor")]
    Override(TypeOverrideArgs),
}

impl TypeCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::List(args) => args.format.format,
        Self::Show(args) => args.format.format,
        Self::Create(args) => args.format.format,
        Self::Override(args) => args.format.format,
        }
    }
}

#[derive(Args, Debug)]
pub struct TypeListArgs {
    #[arg(long, value_name = "ENTRY-LOCATOR")]
    pub at: Option<String>,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct TypeShowArgs {
    #[arg(help = "Locator of a stored Type node")]
    pub locator: String,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct TypeDeclarationArgs {
    #[arg(
        long = "field",
        value_name = "FIELD-DECL",
        help = "Add one field declaration"
    )]
    pub fields: Vec<String>,

    #[arg(long, value_name = "EDGE-TYPE", help = "Permit one outgoing edge type")]
    pub outgoing: Vec<String>,

    #[arg(long, value_name = "EDGE-TYPE", help = "Permit one incoming edge type")]
    pub incoming: Vec<String>,

    #[arg(
        long = "allows-override",
        value_name = "TYPE-NAME",
        help = "Permit one child node type"
    )]
    pub allows_override: Vec<String>,
}

#[derive(Args, Debug)]
pub struct TypeCreateArgs {
    #[arg(help = "Declared type name")]
    pub name: String,

    #[command(flatten)]
    pub declaration: TypeDeclarationArgs,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct TypePatchArgs {
    #[arg(
        long = "field",
        value_name = "FIELD-DECL",
        help = "Replace or append one named field declaration"
    )]
    pub fields: Vec<String>,

    #[arg(
        long = "drop-field",
        value_name = "FIELD-NAME",
        help = "Remove one named field"
    )]
    pub drop_fields: Vec<String>,

    #[arg(
        long = "allow-outgoing",
        value_name = "EDGE-TYPE",
        help = "Add one outgoing edge permission"
    )]
    pub allow_outgoing: Vec<String>,

    #[arg(
        long = "disallow-outgoing",
        value_name = "EDGE-TYPE",
        help = "Remove one outgoing edge permission"
    )]
    pub disallow_outgoing: Vec<String>,

    #[arg(
        long = "allow-incoming",
        value_name = "EDGE-TYPE",
        help = "Add one incoming edge permission"
    )]
    pub allow_incoming: Vec<String>,

    #[arg(
        long = "disallow-incoming",
        value_name = "EDGE-TYPE",
        help = "Remove one incoming edge permission"
    )]
    pub disallow_incoming: Vec<String>,

    #[arg(
        long = "allows-override",
        value_name = "TYPE-NAME",
        help = "Add one permitted child node type"
    )]
    pub allows_override: Vec<String>,

    #[arg(
        long = "disallow-override",
        value_name = "TYPE-NAME",
        help = "Remove one permitted child node type"
    )]
    pub disallow_override: Vec<String>,
}

#[derive(Args, Debug)]
pub struct TypeOverrideArgs {
    #[arg(help = "Unique active declaration name")]
    pub name: String,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        help = "Resolve the source declaration in this historical view"
    )]
    pub at: Option<String>,

    #[arg(
        long = "as",
        value_name = "NEW-NAME",
        help = "Rename the child declaration"
    )]
    pub new_name: Option<String>,

    #[command(flatten)]
    pub patch: TypePatchArgs,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub(crate) struct SelectedTypeOverrideArgs {
    #[arg(
        long = "as",
        value_name = "NEW-NAME",
        help = "Rename the child declaration"
    )]
    pub(crate) new_name: Option<String>,

    #[command(flatten)]
    pub(crate) patch: TypePatchArgs,

    #[command(flatten)]
    pub(crate) transaction: TransactionArgs,
}

#[derive(Subcommand, Debug)]
pub enum SidecarCommand {
    #[command(about = "Persist the absolute path of an existing sidecar")]
    Attach(SidecarAttachArgs),

    #[command(about = "Remove attachment metadata without reading the sidecar")]
    Detach,

    #[command(about = "Preview the analyzed provisional sidecar suffix")]
    Dump(SidecarDumpArgs),

    #[command(about = "Apply and save the rebuilt sidecar suffix")]
    Commit(SidecarCommitArgs),
}

impl SidecarCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::Dump(args) => args.format.format,
        Self::Commit(args) => args.format.format,
        Self::Attach(_) | Self::Detach => OutputFormat::Lines,
        }
    }
}

#[derive(Args, Debug)]
pub struct SidecarAttachArgs {
    #[arg(help = "Path to an existing TOML sidecar")]
    pub path: PathBuf,
}

#[derive(Args, Debug)]
pub struct SidecarDumpArgs {
    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct SidecarCommitArgs {
    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        help = "Commit only if the canonical head still matches this locator"
    )]
    pub expect_head: Option<String>,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Subcommand, Debug)]
pub enum AnalyzerCommand {
    #[command(about = "List configured analyzer name/configuration pairs")]
    List(AnalyzerListArgs),

    #[command(about = "Stage or commit one analyzer configuration")]
    Add(AnalyzerAddArgs),
}

impl AnalyzerCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::List(args) => args.format.format,
        Self::Add(args) => args.format.format,
        }
    }
}

#[derive(Args, Debug)]
pub struct AnalyzerListArgs {
    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct AnalyzerAddArgs {
    #[arg(help = "Registered analyzer name")]
    pub name: String,

    #[arg(help = "Analyzer factory configuration string")]
    pub config: String,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct GraphvizArgs {
    #[arg(help = "DOT or SVG output path")]
    pub target: PathBuf,

    #[arg(
        long,
        value_name = "ENTRY-LOCATOR",
        help = "Render the view through and including this entry"
    )]
    pub at: Option<String>,

    #[arg(long, help = "Include stale entries and node-family relationships")]
    pub full: bool,

    #[arg(long, help = "Include node field values in labels")]
    pub fields: bool,

    #[arg(long, help = "Write DOT directly instead of invoking dot for SVG")]
    pub dot: bool,
}

#[derive(Args, Debug)]
pub struct CheckArgs {
    #[arg(
        long,
        conflicts_with_all = ["only_dismissed", "dismissal_ids"],
        help = "Include emitted dismissed warnings and suggestions"
    )]
    pub include_dismissed: bool,

    #[arg(
        long,
        conflicts_with_all = ["include_dismissed", "dismissal_ids"],
        help = "Show emitted dismissed messages plus every inconsistency"
    )]
    pub only_dismissed: bool,

    #[arg(
        long,
        conflicts_with = "dismissal_ids",
        help = "Include messages whose relevance is resolved by later history"
    )]
    pub include_resolved: bool,

    #[arg(
        long,
        conflicts_with_all = ["include_dismissed", "only_dismissed", "include_resolved"],
        help = "List the persisted dismissal registry instead of emitted messages"
    )]
    pub dismissal_ids: bool,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Subcommand, Debug)]
pub enum MessageCommand {
    #[command(about = "Show one message resolved against the full current universe")]
    Show(MessageIdArgs),

    #[command(about = "Persist dismissal of a warning or suggestion")]
    Dismiss(MessageIdArgs),

    #[command(about = "Remove a persisted dismissal")]
    Restore(MessageIdArgs),

    #[command(about = "Stage or commit a suggestion through the transaction path")]
    Apply(MessageApplyArgs),
}

impl MessageCommand {
    fn output_format(&self) -> OutputFormat {
        match self {
        Self::Show(args) | Self::Dismiss(args) | Self::Restore(args) => args.format.format,
        Self::Apply(args) => args.format.format,
        }
    }
}

#[derive(Args, Debug)]
pub struct MessageIdArgs {
    #[arg(help = "Message ID prefix, beginning with m:")]
    pub message_id: String,

    #[command(flatten)]
    pub format: FormatArgs,
}

#[derive(Args, Debug)]
pub struct MessageApplyArgs {
    #[arg(help = "Suggestion message ID prefix, beginning with m:")]
    pub message_id: String,

    #[command(flatten)]
    pub transaction: TransactionArgs,

    #[arg(long, help = "Do not dismiss the suggestion after a successful commit")]
    pub keep_message: bool,

    #[command(flatten)]
    pub format: FormatArgs,
}
