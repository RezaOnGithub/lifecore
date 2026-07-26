use std::io::{self, Write};

use serde_json::json;

use crate::{
    app::{
        AliasRecord, AnalyzerRecord, CircumstanceRecord, CommandOutcome, EntryRecord,
        QueryExplanationRecord, TypeRecord, ViewSummary,
    },
    cli::OutputFormat,
    error::AppError,
    messages::MessageRecord,
};

pub fn render_outcome(
    outcome: &CommandOutcome,
    writer: &mut dyn Write,
    stdout_is_terminal: bool,
) -> io::Result<()> {
    match outcome {
    CommandOutcome::Text(text) => writer.write_all(text.as_bytes()),
    CommandOutcome::Initialized {
        format,
        path,
        uuid,
        view,
        messages,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            let value = json!({
                "path": path,
                "uuid": uuid,
                "view": view,
                "transaction": {
                    "state": "saved",
                    "entries": [],
                    "messages": messages,
                }
            });
            serde_json::to_writer(&mut *writer, &value)?;
            writeln!(writer)
        } else {
            writeln!(writer, "Initialized {}", path.display())?;
            writeln!(writer, "State UUID: {uuid}")?;
            writeln!(writer, "Transaction: saved")?;
            render_human_messages(messages, view, format, writer)
        }
    }
    CommandOutcome::Messages {
        format,
        view,
        items,
        detail,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            let value = json!({
                "view": view,
                "items": items,
            });
            serde_json::to_writer(&mut *writer, &value)?;
            writeln!(writer)
        } else if *detail {
            render_message_detail(&items[0], view, writer)
        } else {
            render_human_messages(items, view, format, writer)
        }
    }
    CommandOutcome::Dismissals { format, items } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(&mut *writer, &json!({ "items": items }))?;
            writeln!(writer)
        } else {
            if items.is_empty() {
                return writeln!(writer, "No dismissed message IDs.");
            }
            if format == OutputFormat::Table {
                writeln!(writer, "ID\tEMITTED")?;
            }
            for item in items {
                if format == OutputFormat::Table {
                    writeln!(writer, "{}\t{}", item.display_id, item.emitted)?;
                } else {
                    writeln!(writer, "{}  emitted={}", item.id, item.emitted)?;
                }
            }

            Ok(())
        }
    }
    CommandOutcome::DismissalChanged {
        format,
        action,
        id,
        emitted,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "dismissal": {
                        "action": action,
                        "id": id,
                        "emitted": emitted,
                    }
                }),
            )?;
            writeln!(writer)
        } else {
            writeln!(writer, "{} dismissal {} (emitted={emitted})", action, id)
        }
    }
    CommandOutcome::Analyzers {
        format,
        view,
        items,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "items": items,
                }),
            )?;
            writeln!(writer)
        } else {
            render_view(view, writer)?;
            render_analyzers(items, format, writer)
        }
    }
    CommandOutcome::Aliases {
        format,
        view,
        items,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "items": items,
                }),
            )?;
            writeln!(writer)
        } else {
            render_view(view, writer)?;
            render_aliases(items, format, writer)
        }
    }
    CommandOutcome::AliasChanged { format, alias } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(&mut *writer, &json!({ "alias": alias }))?;
            writeln!(writer)
        } else if let Some(locator) = alias.locator.as_deref() {
            writeln!(writer, "{} alias {} -> {locator}", alias.action, alias.name)
        } else {
            writeln!(writer, "{} alias {}", alias.action, alias.name)
        }
    }
    CommandOutcome::SidecarAttachment { action, path } => {
        writeln!(writer, "Sidecar {action}: {}", path.display())
    }
    CommandOutcome::SidecarDump {
        format,
        view,
        items,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "items": items,
                }),
            )?;
            writeln!(writer)
        } else {
            render_view(view, writer)?;
            let entries = items
                .iter()
                .map(|item| item.entry.clone())
                .collect::<Vec<_>>();
            render_entry_listing(&entries, format, writer)
        }
    }
    CommandOutcome::SidecarCommitted { format, result } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "sidecar": {
                        "action": "committed",
                        "attachment_cleared": true,
                    },
                    "view": result.view,
                    "transaction": {
                        "state": result.state,
                        "entries": result.entries,
                        "analyzers": result.analyzers,
                        "messages": result.messages,
                    }
                }),
            )?;
            writeln!(writer)
        } else {
            writeln!(writer, "Sidecar committed; attachment cleared.")?;
            writeln!(writer, "Transaction: {}", result.state.label())?;
            render_view(&result.view, writer)?;
            render_entry_listing(&result.entries, format, writer)?;
            render_human_messages(&result.messages, &result.view, format, writer)
        }
    }
    CommandOutcome::Types {
        format,
        view,
        items,
        detail,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "items": items,
                }),
            )?;
            writeln!(writer)
        } else if *detail {
            render_view(view, writer)?;
            render_type_detail(&items[0], writer)
        } else {
            render_view(view, writer)?;
            render_types(items, format, writer)
        }
    }
    CommandOutcome::Transaction { format, result } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": result.view,
                    "transaction": {
                        "state": result.state,
                        "entries": result.entries,
                        "analyzers": result.analyzers,
                        "messages": result.messages,
                    }
                }),
            )?;
            writeln!(writer)
        } else {
            writeln!(writer, "Transaction: {}", result.state.label())?;
            render_view(&result.view, writer)?;
            render_entry_listing(&result.entries, format, writer)?;
            render_analyzers(&result.analyzers, format, writer)?;
            render_human_messages(&result.messages, &result.view, format, writer)
        }
    }
    CommandOutcome::Entries {
        format,
        view,
        items,
        detail,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "items": items,
                }),
            )?;
            writeln!(writer)
        } else if *detail {
            render_view(view, writer)?;
            render_entry_detail(&items[0], writer)
        } else {
            render_view(view, writer)?;
            render_entry_listing(items, format, writer)
        }
    }
    CommandOutcome::QueryExplanation {
        format,
        view,
        explanation,
    } => {
        let format = resolve_format(*format, stdout_is_terminal);

        if format == OutputFormat::Json {
            serde_json::to_writer(
                &mut *writer,
                &json!({
                    "view": view,
                    "explanation": explanation,
                }),
            )?;
            writeln!(writer)
        } else {
            render_view(view, writer)?;
            render_query_explanation(explanation, writer)
        }
    }
    CommandOutcome::GraphvizArtifact {
        target,
        artifact_kind,
        view,
    } => {
        writeln!(writer, "Wrote {artifact_kind}: {}", target.display())?;
        render_view(view, writer)
    }
    }
}

fn render_types(
    declarations: &[TypeRecord],
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    if declarations.is_empty() {
        return writeln!(writer, "No active, well-formed type declarations.");
    }
    if format == OutputFormat::Table {
        writeln!(writer, "NAME\tLOCATOR\tPROVISIONAL")?;
    }
    for declaration in declarations {
        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}",
                declaration.name, declaration.display_locator, declaration.provisional
            )?;
        } else {
            writeln!(
                writer,
                "{}  {}  provisional={}",
                declaration.name, declaration.locator, declaration.provisional
            )?;
        }
    }

    Ok(())
}

fn render_type_detail(declaration: &TypeRecord, writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer, "Locator: {}", declaration.locator)?;
    writeln!(writer, "Display locator: {}", declaration.display_locator)?;
    writeln!(writer, "Provisional: {}", declaration.provisional)?;
    writeln!(writer, "Declaration:")?;
    writeln!(writer, "{}", declaration.command)
}

fn render_aliases(
    aliases: &[AliasRecord],
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    if aliases.is_empty() {
        return writeln!(writer, "No aliases.");
    }
    if format == OutputFormat::Table {
        writeln!(writer, "NAME\tLOCATOR\tSTATUS\tTEMPORARY")?;
    }
    for alias in aliases {
        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}",
                alias.name,
                alias.display_locator,
                alias.status.label(),
                alias.temporary
            )?;
        } else {
            writeln!(
                writer,
                "{}  {}  status={}  temporary={}",
                alias.name,
                alias.locator,
                alias.status.label(),
                alias.temporary
            )?;
        }
    }

    Ok(())
}

pub fn render_error(error: &AppError, writer: &mut dyn Write) -> io::Result<()> {
    if error.output_format() == OutputFormat::Json {
        let mut detail = json!({
            "kind": error.kind(),
            "message": error.to_string(),
        });

        match error {
        AppError::SidecarUnavailable {
            operation,
            path,
            reason,
            ..
        } => {
            detail["operation"] = json!(operation);
            detail["path"] = json!(path);
            detail["reason"] = json!(reason);
            detail["recovery"] = json!("lifecore sidecar detach");
        }
        AppError::MutationWithSidecar { operation, .. }
        | AppError::SidecarDiagnosable { operation, .. } => {
            detail["operation"] = json!(operation);
            detail["recovery"] = json!("lifecore sidecar detach");
        }
        AppError::SidecarAlreadyAttached { path, .. } => {
            detail["path"] = json!(path);
            detail["recovery"] = json!("lifecore sidecar detach");
        }
        _ => {}
        }
        if let AppError::TransactionAborted {
            detail: transaction_detail,
            ..
        } = error
        {
            detail["view"] = json!(transaction_detail.view);
            detail["transaction"] = json!({
                "state": "aborted",
                "entries": transaction_detail.entries,
                "analyzers": transaction_detail.analyzers,
                "messages": transaction_detail.messages,
            });
        }

        serde_json::to_writer(&mut *writer, &json!({ "error": detail }))?;
        writeln!(writer)
    } else {
        match error {
        AppError::Usage { message, .. } => writer.write_all(message.as_bytes()),
        AppError::TransactionAborted { detail, .. } => {
            writeln!(writer, "error: {error}")?;
            writeln!(writer, "Transaction: aborted")?;
            render_view(&detail.view, writer)?;
            render_entry_listing(&detail.entries, OutputFormat::Lines, writer)?;
            render_analyzers(&detail.analyzers, OutputFormat::Lines, writer)?;
            render_human_messages(&detail.messages, &detail.view, OutputFormat::Lines, writer)
        }
        _ => writeln!(writer, "error: {error}"),
        }
    }
}

fn render_analyzers(
    analyzers: &[AnalyzerRecord],
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    if analyzers.is_empty() {
        return Ok(());
    }
    if format == OutputFormat::Table {
        writeln!(writer, "ANALYZER\tCONFIG")?;
    }
    for analyzer in analyzers {
        if format == OutputFormat::Table {
            writeln!(writer, "{}\t{}", analyzer.name, analyzer.config)?;
        } else {
            writeln!(writer, "{}  config={:?}", analyzer.name, analyzer.config)?;
        }
    }

    Ok(())
}

fn resolve_format(format: OutputFormat, stdout_is_terminal: bool) -> OutputFormat {
    match format {
    OutputFormat::Auto if stdout_is_terminal => OutputFormat::Table,
    OutputFormat::Auto => OutputFormat::Lines,
    selected => selected,
    }
}

fn render_human_messages(
    messages: &[MessageRecord],
    view: &ViewSummary,
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    if messages.is_empty() {
        return writeln!(writer, "No messages in view 0..{}.", view.end);
    }

    if format == OutputFormat::Table {
        writeln!(
            writer,
            "ID\tSEVERITY\tANALYZER\tLOCATION\tSTATE\tRELEVANCE\tMESSAGE\tACTION"
        )?;
    }

    for message in messages {
        let action = message.action_summary();
        let state = message_presentation_state(message);
        let relevance = message
            .relevance
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");

        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                message.display_id,
                message.severity.label(),
                message.analyzer,
                message.location.display(),
                state,
                relevance,
                message.text,
                action
            )?;
        } else {
            write!(
                writer,
                "{}  {}  {}  {}  {}",
                message.display_id,
                message.severity.label(),
                message.analyzer,
                message.location.display(),
                message.text,
            )?;
            write!(writer, "  state={state}")?;
            if !relevance.is_empty() {
                write!(writer, "  relevance={relevance}")?;
            }

            if !action.is_empty() {
                write!(writer, "  action: {action}")?;
            }

            writeln!(writer)?;
        }
    }

    Ok(())
}

fn render_message_detail(
    message: &MessageRecord,
    view: &ViewSummary,
    writer: &mut dyn Write,
) -> io::Result<()> {
    writeln!(writer, "ID: m:{}", message.id)?;
    writeln!(writer, "Display ID: {}", message.display_id)?;
    writeln!(writer, "Severity: {}", message.severity.label())?;
    writeln!(
        writer,
        "Analyzer: {} ({})",
        message.analyzer, message.analyzer_config
    )?;
    writeln!(writer, "Location: {}", message.location.display())?;
    writeln!(writer, "View: 0..{}", view.end)?;
    writeln!(writer, "Dismissed: {}", message.dismissed)?;
    writeln!(writer, "Resolved: {}", message.resolved)?;
    writeln!(writer, "Visible: {}", message.visible)?;
    writeln!(
        writer,
        "Relevance: {}",
        message
            .relevance
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    writeln!(writer, "Message: {}", message.text)?;

    for (index, action) in message.actions.iter().enumerate() {
        writeln!(writer, "Action {}: {}", index + 1, action.summary())?;
    }

    Ok(())
}

fn message_presentation_state(message: &MessageRecord) -> &'static str {
    match (message.dismissed, message.resolved) {
    (false, false) => "active",
    (true, false) => "dismissed",
    (false, true) => "resolved",
    (true, true) => "dismissed,resolved",
    }
}

fn render_view(view: &ViewSummary, writer: &mut dyn Write) -> io::Result<()> {
    match &view.display_through {
    Some(locator) => writeln!(
        writer,
        "View: through {locator} (0..{}; canonical boundary {})",
        view.end, view.canonical_end
    ),
    None => writeln!(writer, "View: empty (0..0; canonical boundary 0)"),
    }
}

fn render_entry_listing(
    entries: &[EntryRecord],
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    if entries.is_empty() {
        return writeln!(writer, "No entries.");
    }

    if format == OutputFormat::Table {
        writeln!(
            writer,
            "LOCATOR\tKIND\tTYPE\tFRESH\tPROVISIONAL\tALIASES\tTEMPORARY_ALIASES"
        )?;
    }

    for entry in entries {
        let aliases = entry.aliases.join(",");
        let temporary_aliases = entry.temporary_aliases.join(",");
        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                entry.display_locator,
                entry.kind,
                entry.type_name,
                entry.fresh,
                entry.provisional,
                aliases,
                temporary_aliases
            )?;
        } else {
            write!(
                writer,
                "{}  {}  {}  fresh={}  provisional={}",
                entry.display_locator, entry.kind, entry.type_name, entry.fresh, entry.provisional
            )?;
            if !aliases.is_empty() {
                write!(writer, "  aliases={aliases}")?;
            }
            if !temporary_aliases.is_empty() {
                write!(writer, "  temporary_aliases={temporary_aliases}")?;
            }
            writeln!(writer)?;
        }
    }

    Ok(())
}

fn render_entry_detail(entry: &EntryRecord, writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer, "Locator: {}", entry.locator)?;
    writeln!(writer, "Display locator: {}", entry.display_locator)?;
    writeln!(writer, "Index: {}", entry.index)?;
    writeln!(writer, "Kind: {}", entry.kind)?;
    writeln!(writer, "Type: {}", entry.type_name)?;
    writeln!(writer, "Fresh: {}", entry.fresh)?;
    writeln!(writer, "Provisional: {}", entry.provisional)?;
    writeln!(writer, "Aliases: {}", entry.aliases.join(", "))?;
    writeln!(
        writer,
        "Temporary aliases: {}",
        entry.temporary_aliases.join(", ")
    )?;

    if let Some(parent) = &entry.parent {
        writeln!(writer, "Parent: {parent}")?;
    }
    if let Some(fields) = &entry.fields {
        writeln!(
            writer,
            "Fields: {}",
            serde_json::to_string(fields).expect("serializable entry fields")
        )?;
    }
    if let Some(from) = &entry.from {
        writeln!(writer, "From: {from}")?;
    }
    if let Some(to) = &entry.to {
        writeln!(writer, "To: {to}")?;
    }

    match &entry.circumstance {
    CircumstanceRecord::Mock => writeln!(writer, "Circumstance: Mock"),
    CircumstanceRecord::Session {
        when_1h,
        hostname,
        username,
    } => writeln!(
        writer,
        "Circumstance: Session {when_1h} {username}@{hostname}"
    ),
    }
}

fn render_query_explanation(
    explanation: &QueryExplanationRecord,
    writer: &mut dyn Write,
) -> io::Result<()> {
    writeln!(
        writer,
        "Candidate: {}",
        explanation.candidate.display_locator
    )?;
    writeln!(writer, "Combinator: {}", explanation.combinator)?;
    for predicate in &explanation.predicates {
        writeln!(writer, "{}\t{}", predicate.matches, predicate.predicate)?;
    }
    writeln!(writer, "Matches: {}", explanation.matches)
}
