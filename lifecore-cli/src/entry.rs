use lifecore::{Circumstance, Entry, LifeCoreAccess, LifeCoreList};

use crate::{
    app::{CircumstanceRecord, EntryRecord},
    error::AppError,
    locator::{EntryExpectation, ResolvedEntry, resolve_index},
    state::CliStateV1,
    view::GraphView,
};

pub fn entry_record(
    graph: &LifeCoreList,
    state: &CliStateV1,
    view: &GraphView,
    resolved: &ResolvedEntry,
) -> Result<EntryRecord, AppError> {
    let index = resolved.index();
    if index >= view.end_exclusive() {
        return Err(AppError::entry_outside_view(index, view.end_exclusive()));
    }

    //A Freshness is contextual, so re-point the verified absolute identity
    //A into this command's bounded view before asking core.
    let pointed = view
        .context()
        .look_at(resolved.entry.entry_context().absolute());
    let fresh = !graph
        .is_stale(pointed)
        .ok_or_else(|| AppError::invalid_state("resolved entry has no freshness result"))?;
    let aliases = state
        .aliases
        .iter()
        .filter(|(_, locator)| {
            usize::try_from(locator.index).ok() == Some(index)
                && locator.digest == resolved.fingerprint.as_str()
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let circumstance = match &resolved.circumstance {
    Circumstance::Mock => CircumstanceRecord::Mock,
    Circumstance::Session {
        when_1h,
        hostname,
        username,
    } => CircumstanceRecord::Session {
        when_1h: when_1h.to_string(),
        hostname: hostname.clone(),
        username: username.clone(),
    },
    };

    match &resolved.entry {
    Entry::Node(node) => {
        let parent = node
            .parent
            .map(|parent| {
                resolve_index(
                    graph,
                    state,
                    parent.0,
                    EntryExpectation::Node,
                    Some(view.end_exclusive()),
                )
                .map(|entry| entry.full_locator())
            })
            .transpose()?;

        Ok(EntryRecord {
            index,
            locator: resolved.full_locator(),
            display_locator: resolved.display_locator(),
            kind: "node".to_string(),
            type_name: node.ty_name.clone(),
            aliases,
            temporary_aliases: Vec::new(),
            fresh,
            provisional: matches!(&circumstance, CircumstanceRecord::Mock),
            parent,
            fields: Some(node.field.clone()),
            from: None,
            to: None,
            circumstance,
        })
    }
    Entry::Edge(edge) => {
        let from = resolve_index(
            graph,
            state,
            edge.from.0,
            EntryExpectation::Node,
            Some(view.end_exclusive()),
        )?
        .full_locator();
        let to = resolve_index(
            graph,
            state,
            edge.to.0,
            EntryExpectation::Node,
            Some(view.end_exclusive()),
        )?
        .full_locator();

        Ok(EntryRecord {
            index,
            locator: resolved.full_locator(),
            display_locator: resolved.display_locator(),
            kind: "edge".to_string(),
            type_name: edge.ty_name.clone(),
            aliases,
            temporary_aliases: Vec::new(),
            fresh,
            provisional: matches!(&circumstance, CircumstanceRecord::Mock),
            parent: None,
            fields: None,
            from: Some(from),
            to: Some(to),
            circumstance,
        })
    }
    }
}
