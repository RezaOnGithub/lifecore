use std::{
    collections::BTreeMap,
    fmt,
    ops::{Deref, DerefMut},
    str::FromStr,
};

use lifecore::{EntryIndex, LifeCore, LifeCoreList};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppError,
    locator::{
        EntryExpectation, EntryLocator, ResolvedEntry, resolve_entry, resolve_stored_locator,
    },
    state::{CliStateV1, StoredEntryLocatorV1},
    view::GraphView,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AliasName(String);

impl AliasName {
    pub fn parse(text: &str) -> Result<Self, AppError> {
        validate_alias(text)?;

        Ok(Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AliasName {
    type Err = AppError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl fmt::Display for AliasName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AliasRegistry(BTreeMap<String, StoredEntryLocatorV1>);

impl AliasRegistry {
    pub fn bind(
        &mut self,
        name: AliasName,
        locator: StoredEntryLocatorV1,
    ) -> Option<StoredEntryLocatorV1> {
        self.0.insert(name.0, locator)
    }

    pub fn remove_name(&mut self, name: &AliasName) -> Option<StoredEntryLocatorV1> {
        self.0.remove(name.as_str())
    }
}

impl Deref for AliasRegistry {
    type Target = BTreeMap<String, StoredEntryLocatorV1>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AliasRegistry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasStatus {
    Current,
    Stale,
    OutsideView,
}

impl AliasStatus {
    pub fn label(self) -> &'static str {
        match self {
        Self::Current => "current",
        Self::Stale => "stale",
        Self::OutsideView => "outside-view",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeReference {
    Head,
    Alias(AliasName),
    Locator(EntryLocator),
}

impl NodeReference {
    pub fn parse(text: &str) -> Result<Self, AppError> {
        //A A colon commits the input to locator parsing so a bad fingerprint
        //A can never be reinterpreted as an alias.
        if text.contains(':') {
            return EntryLocator::parse(text).map(Self::Locator);
        }

        if text == "HEAD" {
            return Ok(Self::Head);
        }

        AliasName::parse(text).map(Self::Alias)
    }
}

pub fn validate_alias(name: &str) -> Result<(), AppError> {
    if name == "HEAD" {
        return Err(AppError::invalid_alias("HEAD is reserved"));
    }
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return Err(AppError::invalid_alias("alias cannot be empty"));
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(AppError::invalid_alias("expected [A-Za-z_][A-Za-z0-9_]*"));
    }

    Ok(())
}

pub fn resolve_node_reference(
    graph: &LifeCoreList,
    state: &CliStateV1,
    reference: &NodeReference,
    view: &GraphView,
) -> Result<ResolvedEntry, AppError> {
    match reference {
    NodeReference::Locator(locator) => resolve_entry(
        graph,
        state,
        locator,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    ),
    NodeReference::Alias(name) => {
        let locator = state.aliases.get(name.as_str()).ok_or_else(|| {
            AppError::invalid_node_reference(format!("unknown alias {:?}", name.as_str()))
        })?;
        resolve_stored_locator(
            graph,
            state,
            locator,
            EntryExpectation::Node,
            Some(view.end_exclusive()),
        )
    }
    NodeReference::Head => {
        //A HEAD is the newest visible node, not the view's final entry;
        //A scanning backward deliberately skips a trailing edge.
        for index in (0..view.end_exclusive()).rev() {
            let context = view
                .context()
                .look_at(EntryIndex(index).before_and_including());
            if graph.node(context).is_some() {
                let entry = graph.get(context).ok_or_else(|| {
                    AppError::invalid_state("visible HEAD candidate could not be loaded")
                })?;
                let fingerprint =
                    crate::locator::EntryFingerprint::for_entry(state, context.entry(), &entry)?;

                return resolve_entry(
                    graph,
                    state,
                    &EntryLocator::parse(&format!("{index}:{}", fingerprint.as_str()))?,
                    EntryExpectation::Node,
                    Some(view.end_exclusive()),
                );
            }
        }

        Err(AppError::invalid_node_reference(
            "HEAD cannot resolve because the selected view contains no nodes",
        ))
    }
    }
}

pub fn alias_status(
    graph: &LifeCoreList,
    state: &CliStateV1,
    target: &StoredEntryLocatorV1,
    view: &GraphView,
) -> Result<(ResolvedEntry, AliasStatus), AppError> {
    //A Validate durable identity and node kind independently of the selected
    //A view so a legitimate future target can be reported as outside-view.
    let resolved = resolve_stored_locator(graph, state, target, EntryExpectation::Node, None)?;
    if resolved.index() >= view.end_exclusive() {
        return Ok((resolved, AliasStatus::OutsideView));
    }

    let pointed = view
        .context()
        .look_at(resolved.entry.entry_context().absolute());
    let stale = graph
        .is_stale(pointed)
        .ok_or_else(|| AppError::invalid_state("alias target has no freshness result"))?;
    let status = if stale {
        AliasStatus::Stale
    } else {
        AliasStatus::Current
    };

    Ok((resolved, status))
}

pub fn visible_aliases(
    graph: &LifeCoreList,
    state: &CliStateV1,
    view: &GraphView,
) -> Result<BTreeMap<usize, Vec<String>>, AppError> {
    let mut visible = BTreeMap::<usize, Vec<String>>::new();
    for (name, target) in state.aliases.iter() {
        let (resolved, status) = alias_status(graph, state, target, view)?;
        if status != AliasStatus::OutsideView {
            visible
                .entry(resolved.index())
                .or_default()
                .push(name.clone());
        }
    }

    Ok(visible)
}
