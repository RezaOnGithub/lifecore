use lifecore::{EntryIndex, LifeCore, LifeCoreList};

use crate::{
    error::AppError,
    locator::{
        EntryExpectation, EntryLocator, ResolvedEntry, resolve_entry, resolve_stored_locator,
    },
    state::CliStateV1,
    view::GraphView,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeReference {
    Head,
    Alias(String),
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

        validate_alias(text)?;
        Ok(Self::Alias(text.to_string()))
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
        let locator = state
            .aliases
            .get(name)
            .ok_or_else(|| AppError::invalid_node_reference(format!("unknown alias {name:?}")))?;
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
