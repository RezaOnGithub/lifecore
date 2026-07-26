use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use lifecore::{Body, EntryIndex, LifeCore};
use serde::{Deserialize, Serialize};

use crate::{
    app::EntryRecord,
    error::AppError,
    locator::{EntryExpectation, resolve_index},
    payload_input::payload_from_toml,
    reference::{AliasName, NodeReference, resolve_node_reference},
    state::{PersistedState, StateStore, StoredEntryLocatorV1},
    transaction::{PlannedChange, stage_change},
    view::GraphView,
};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarDocument {
    #[serde(default)]
    pub entry: Vec<SidecarEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarEntry {
    pub alias: Option<String>,
    pub ty: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub body: toml::Table,
    #[serde(default)]
    pub connect: SidecarConnections,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarConnections {
    #[serde(default)]
    pub connect_to: Vec<SidecarConnection>,
    #[serde(default)]
    pub connect_back_from: Vec<SidecarConnection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarConnection {
    pub target: String,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct SidecarPlan {
    pub change: PlannedChange,
    pub temporary_aliases: BTreeMap<String, StoredEntryLocatorV1>,
}

#[derive(Clone)]
pub struct BuiltSidecar {
    pub plan: SidecarPlan,
    pub staged: PersistedState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SidecarDumpItem {
    #[serde(flatten)]
    pub entry: EntryRecord,
    pub temporary_aliases: Vec<String>,
}

pub fn parse_sidecar(bytes: &[u8]) -> Result<SidecarDocument, AppError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| AppError::invalid_sidecar(format!("source is not UTF-8: {error}")))?;

    toml::from_str(text).map_err(|error| AppError::invalid_sidecar(error.to_string()))
}

pub fn read_sidecar(path: &Path) -> Result<Vec<u8>, AppError> {
    fs::read(path).map_err(|error| AppError::sidecar_io(path, "read", error))
}

pub fn attach_path(
    store: &StateStore,
    state: &PersistedState,
    requested: &Path,
    invocation_directory: &Path,
) -> Result<PathBuf, AppError> {
    if let Some(attached) = state.cli.sidecar_path.as_ref() {
        return Err(AppError::sidecar_already_attached(attached));
    }

    let selected = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        invocation_directory.join(requested)
    };
    let absolute = fs::canonicalize(&selected)
        .map_err(|error| AppError::sidecar_io(&selected, "resolve", error))?;
    if !absolute.is_file() {
        return Err(AppError::invalid_sidecar(format!(
            "attachment path {} is not a file",
            absolute.display()
        )));
    }

    let mut candidate = state.clone();
    candidate.cli.sidecar_path = Some(absolute.clone());
    store.save(&candidate)?;

    Ok(absolute)
}

pub fn detach(store: &StateStore, state: &PersistedState) -> Result<PathBuf, AppError> {
    let attached = state
        .cli
        .sidecar_path
        .clone()
        .ok_or_else(AppError::sidecar_not_attached)?;
    let mut candidate = state.clone();
    candidate.cli.sidecar_path = None;
    store.save(&candidate)?;

    Ok(attached)
}

pub fn build_sidecar_plan(
    canonical: &PersistedState,
    document: SidecarDocument,
) -> Result<BuiltSidecar, AppError> {
    let mut staged = canonical.clone();
    let mut changes = Vec::new();
    let mut temporary_aliases = BTreeMap::new();

    for (source_index, entry) in document.entry.into_iter().enumerate() {
        //A HEAD is captured before the node and is reused for every HEAD token
        //A in this source entry, including connections staged after the node.
        let captured_head = capture_head(&staged)?;
        let body = convert_body(entry.body).map_err(|error| {
            AppError::invalid_sidecar(format!("entry {} body: {error}", source_index + 1))
        })?;
        let parent = entry
            .parent
            .as_deref()
            .map(NodeReference::parse)
            .transpose()
            .map_err(|error| sidecar_reference_error(source_index, "parent", error))?
            .as_ref()
            .map(|reference| {
                resolve_sidecar_reference(&staged, reference, captured_head.as_ref())
                    .map(|resolved| EntryIndex(resolved.index()))
            })
            .transpose()
            .map_err(|error| sidecar_reference_error(source_index, "parent", error))?;
        let node_change = PlannedChange::AddNode {
            type_name: entry.ty,
            body,
            parent,
        };
        stage_change(&mut staged, &node_change)
            .map_err(|error| sidecar_stage_error(source_index, "node", error))?;
        changes.push(node_change);

        let node_index = staged
            .lifecore
            .last()
            .ok_or_else(|| AppError::invalid_sidecar("staged node did not produce an entry"))?
            .entry();

        if let Some(alias) = entry.alias {
            let alias = AliasName::parse(&alias).map_err(|error| {
                AppError::invalid_sidecar(format!(
                    "entry {} temporary alias: {error}",
                    source_index + 1
                ))
            })?;
            if staged.cli.aliases.contains_key(alias.as_str()) {
                return Err(AppError::invalid_sidecar(format!(
                    "entry {} temporary alias {:?} shadows a persisted or earlier temporary alias",
                    source_index + 1,
                    alias.as_str()
                )));
            }
            let resolved = resolve_index(
                &staged.lifecore,
                &staged.cli,
                node_index.0,
                EntryExpectation::Node,
                None,
            )?;
            let locator = resolved.durable_locator();
            staged.cli.aliases.bind(alias.clone(), locator.clone());
            temporary_aliases.insert(alias.to_string(), locator);
        }

        //A Non-HEAD connection references intentionally resolve after the
        //A temporary alias is installed, making explicit self-links possible.
        for connection in entry.connect.connect_to {
            let target = resolve_connection_target(
                &staged,
                source_index,
                &connection.target,
                captured_head.as_ref(),
            )?;
            let change = PlannedChange::AddEdge {
                from: node_index,
                type_name: connection.ty,
                to: EntryIndex(target),
            };
            stage_change(&mut staged, &change)
                .map_err(|error| sidecar_stage_error(source_index, "connect_to", error))?;
            changes.push(change);
        }

        for connection in entry.connect.connect_back_from {
            let target = resolve_connection_target(
                &staged,
                source_index,
                &connection.target,
                captured_head.as_ref(),
            )?;
            let change = PlannedChange::AddEdge {
                from: EntryIndex(target),
                type_name: connection.ty,
                to: node_index,
            };
            stage_change(&mut staged, &change)
                .map_err(|error| sidecar_stage_error(source_index, "connect_back_from", error))?;
            changes.push(change);
        }
    }

    staged.lifecore.basic_validation().map_err(|error| {
        AppError::invalid_sidecar(format!("basic validation failed: {error:?}"))
    })?;

    Ok(BuiltSidecar {
        plan: SidecarPlan {
            change: PlannedChange::Composite(changes),
            temporary_aliases,
        },
        staged,
    })
}

fn convert_body(table: toml::Table) -> Result<Body, AppError> {
    let mut body = Body::new();
    for (name, value) in table {
        body.insert(name, payload_from_toml(&value)?.into_payload()?);
    }

    Ok(body)
}

fn capture_head(state: &PersistedState) -> Result<Option<crate::locator::ResolvedEntry>, AppError> {
    let view = GraphView::latest(&state.lifecore, &state.cli)?;
    match resolve_node_reference(&state.lifecore, &state.cli, &NodeReference::Head, &view) {
    Ok(resolved) => Ok(Some(resolved)),
    Err(AppError::InvalidNodeReference { .. }) if view.end_exclusive() == 0 => Ok(None),
    Err(error) => Err(error),
    }
}

fn resolve_sidecar_reference(
    state: &PersistedState,
    reference: &NodeReference,
    captured_head: Option<&crate::locator::ResolvedEntry>,
) -> Result<crate::locator::ResolvedEntry, AppError> {
    if matches!(reference, NodeReference::Head) {
        return captured_head.cloned().ok_or_else(|| {
            AppError::invalid_node_reference(
                "HEAD cannot resolve because the pre-entry graph contains no nodes",
            )
        });
    }

    let view = GraphView::latest(&state.lifecore, &state.cli)?;
    resolve_node_reference(&state.lifecore, &state.cli, reference, &view)
}

fn resolve_connection_target(
    state: &PersistedState,
    source_index: usize,
    text: &str,
    captured_head: Option<&crate::locator::ResolvedEntry>,
) -> Result<usize, AppError> {
    let reference = NodeReference::parse(text)
        .map_err(|error| sidecar_reference_error(source_index, "connection", error))?;
    resolve_sidecar_reference(state, &reference, captured_head)
        .map(|resolved| resolved.index())
        .map_err(|error| sidecar_reference_error(source_index, "connection", error))
}

fn sidecar_reference_error(source_index: usize, field: &str, error: AppError) -> AppError {
    AppError::invalid_sidecar(format!(
        "entry {} {field} reference: {error}",
        source_index + 1
    ))
}

fn sidecar_stage_error(source_index: usize, phase: &str, error: AppError) -> AppError {
    AppError::invalid_sidecar(format!(
        "entry {} {phase} staging failed: {error}",
        source_index + 1
    ))
}
