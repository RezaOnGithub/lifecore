use std::io::{self, Write};

use serde_json::json;

use crate::{
    app::{CircumstanceRecord, CommandOutcome, EntryRecord, QueryExplanationRecord, ViewSummary},
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
    }
}

pub fn render_error(error: &AppError, writer: &mut dyn Write) -> io::Result<()> {
    if error.output_format() == OutputFormat::Json {
        let mut detail = json!({
            "kind": error.kind(),
            "message": error.to_string(),
        });

        if let AppError::Unavailable { operation, .. } = error {
            detail["operation"] = json!(operation);
        }

        serde_json::to_writer(&mut *writer, &json!({ "error": detail }))?;
        writeln!(writer)
    } else {
        match error {
        AppError::Usage { message, .. } => writer.write_all(message.as_bytes()),
        _ => writeln!(writer, "error: {error}"),
        }
    }
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
        writeln!(writer, "ID\tSEVERITY\tANALYZER\tLOCATION\tMESSAGE\tACTION")?;
    }

    for message in messages {
        let action = message.action_summary();

        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}",
                message.display_id,
                message.severity.label(),
                message.analyzer,
                message.location.display(),
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
                message.text
            )?;

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
    writeln!(writer, "Message: {}", message.text)?;

    for (index, action) in message.actions.iter().enumerate() {
        writeln!(writer, "Action {}: {}", index + 1, action.summary())?;
    }

    Ok(())
}

fn render_view(view: &ViewSummary, writer: &mut dyn Write) -> io::Result<()> {
    match &view.display_through {
    Some(locator) => writeln!(writer, "View: through {locator} (0..{})", view.end),
    None => writeln!(writer, "View: empty (0..0)"),
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
        writeln!(writer, "LOCATOR\tKIND\tTYPE\tFRESH\tALIASES")?;
    }

    for entry in entries {
        let aliases = entry.aliases.join(",");
        if format == OutputFormat::Table {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}",
                entry.display_locator, entry.kind, entry.type_name, entry.fresh, aliases
            )?;
        } else {
            write!(
                writer,
                "{}  {}  {}  fresh={}",
                entry.display_locator, entry.kind, entry.type_name, entry.fresh
            )?;
            if !aliases.is_empty() {
                write!(writer, "  aliases={aliases}")?;
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
    writeln!(writer, "Aliases: {}", entry.aliases.join(", "))?;

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
