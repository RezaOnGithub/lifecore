use std::io::{self, Write};

use serde_json::json;

use crate::{
    app::{CommandOutcome, ViewSummary},
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
