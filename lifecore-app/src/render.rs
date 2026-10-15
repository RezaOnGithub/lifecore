use crate::{AppError, query::ViewQuery};
use lifecore::{Entry, Node, Payload, inspection::dot_quote};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderFormat {
    Dot,
    Svg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
    pub format: RenderFormat,
    pub text: String,
}

// The client's row label limit (INTERFACES 5.2), so a rendered node reads
// like its table row.
const LABEL_LIMIT: usize = 60;
const GRAPHVIZ: &str = "dot";
const GRAPHVIZ_TIMEOUT: Duration = Duration::from_secs(10);
const GRAPHVIZ_POLL: Duration = Duration::from_millis(20);
const SVG_LIMIT: usize = 16 * 1024 * 1024;
// Enough of stderr to explain a failure without carrying a runaway log.
const STDERR_LIMIT: u64 = 64 * 1024;

pub(crate) fn render(query: &ViewQuery, format: RenderFormat) -> Result<Rendered, AppError> {
    let dot = to_dot(query);
    let text = match format {
    RenderFormat::Dot => dot,
    RenderFormat::Svg => graphviz(GRAPHVIZ, &dot)?,
    };
    Ok(Rendered { format, text })
}

// The payload text a row is labeled by, as the client labels it (INTERFACES
// 5.2): `title`, then `name`, then the alphabetically first top-level text
// field; blank text does not count.
fn label_text(node: &Node) -> Option<&str> {
    let text = |field: &str| match node.payload.get(field) {
    Some(Payload::Text(text)) if !text.trim().is_empty() => Some(text.as_str()),
    _ => None,
    };
    text("title")
        .or_else(|| text("name"))
        .or_else(|| node.payload.keys().find_map(|field| text(field)))
}

fn row_label(node: &Node) -> String {
    let Some(text) = label_text(node) else {
        return format!("{} @{}", node.ty_name, node.id.position());
    };
    let mut chars = text.chars();
    let cut = chars.by_ref().take(LABEL_LIMIT - 1).collect::<String>();
    if chars.next().is_none() {
        text.to_string()
    } else {
        format!("{cut}\u{2026}")
    }
}

// The queried subgraph in the style of `View::to_dot`: nodes in append
// order, then revision links between result nodes, then the result edges.
// Every node and edge carries its position as `id` and an inspector `href`,
// which Graphviz keeps in SVG output, so the picture is clickable.
pub(crate) fn to_dot(query: &ViewQuery) -> String {
    let mut dot = String::from("digraph lifecore {\n");
    writeln!(
        dot,
        "  label=\"{} boundary {}\";",
        query.view.database, query.view.end_exclusive
    )
    .unwrap();
    let nodes = query
        .nodes
        .iter()
        .filter_map(|queried| match &queried.node.entry {
        Entry::Node(node) => Some((node, queried.node.current)),
        Entry::Edge(_) => None,
        })
        .collect::<Vec<_>>();
    let present = nodes
        .iter()
        .map(|(node, _)| node.id)
        .collect::<BTreeSet<_>>();
    for &(node, current) in &nodes {
        let label = format!(
            "{}\n@{} {}",
            row_label(node),
            node.id.position(),
            node.ty_name
        );
        writeln!(
            dot,
            "  n{} [label={}, id=\"n{}\", href=\"#inspect/{}/{}\"{}];",
            node.id.position(),
            dot_quote(&label),
            node.id.position(),
            node.id.origin(),
            node.id.position(),
            if current { "" } else { ", color=gray" }
        )
        .unwrap();
    }
    for &(node, _) in &nodes {
        if let Some(predecessor) = node.predecessor.filter(|id| present.contains(id)) {
            writeln!(
                dot,
                "  n{} -> n{} [label=\"revision\", style=dashed, color=blue];",
                predecessor.position(),
                node.id.position()
            )
            .unwrap();
        }
    }
    for record in &query.edges {
        let Entry::Edge(edge) = &record.entry else {
            continue;
        };
        let label = format!("@{} {}", edge.id.position(), edge.ty_name);
        writeln!(
            dot,
            "  n{} -> n{} [label={}, id=\"e{}\", href=\"#inspect/{}/{}\", style=solid{}];",
            edge.from.position(),
            edge.to.position(),
            dot_quote(&label),
            edge.id.position(),
            edge.id.origin(),
            edge.id.position(),
            if record.current { "" } else { ", color=gray" }
        )
        .unwrap();
    }
    dot.push_str("}\n");
    dot
}

// Reads up to the cap and then keeps draining, so an oversized render is
// reported from the child's exit rather than from a pipe it can no longer
// write to.
fn read_capped(mut pipe: impl Read, limit: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    pipe.by_ref().take(limit + 1).read_to_end(&mut bytes).ok();
    io::copy(&mut pipe, &mut io::sink()).ok();
    bytes
}

// Runs `<program> -Tsvg` synchronously with the DOT text on stdin and a
// deadline. Each pipe has its own thread, so a full pipe never blocks the
// polling loop, and the child is killed when the deadline passes.
pub(crate) fn graphviz(program: &str, dot: &str) -> Result<String, AppError> {
    let mut child = Command::new(program)
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => AppError::GraphvizUnavailable {
            message: error.to_string(),
        },
        _ => AppError::GraphvizFailed {
            message: format!("cannot start {program}: {error}"),
        },
        })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let input = dot.to_owned();
    // A failed write means the child stopped reading; its exit status says why.
    let writer = thread::spawn(move || {
        stdin.write_all(input.as_bytes()).ok();
    });
    let output = thread::spawn(move || read_capped(stdout, SVG_LIMIT as u64));
    let errors = thread::spawn(move || read_capped(stderr, STDERR_LIMIT));
    let deadline = Instant::now() + GRAPHVIZ_TIMEOUT;
    let status = loop {
        match child.try_wait() {
        Ok(Some(status)) => break Ok(status),
        Ok(None) if Instant::now() < deadline => thread::sleep(GRAPHVIZ_POLL),
        Ok(None) => {
            child.kill().ok();
            child.wait().ok();
            break Err(format!(
                "{program} did not finish within {} s",
                GRAPHVIZ_TIMEOUT.as_secs()
            ));
        }
        Err(error) => {
            child.kill().ok();
            child.wait().ok();
            break Err(format!("cannot wait for {program}: {error}"));
        }
        }
    };
    // Killing the child closes its pipes, so every thread finishes.
    writer.join().ok();
    let svg = output.join().unwrap_or_default();
    let stderr = errors.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr);
    let failed = |message: String| AppError::GraphvizFailed {
        message: match stderr.trim() {
        "" => message,
        stderr => format!("{message}; stderr: {stderr}"),
        },
    };
    let status = status.map_err(failed)?;
    if !status.success() {
        return Err(failed(format!("{program} exited with {status}")));
    }
    if svg.len() > SVG_LIMIT {
        return Err(failed(format!(
            "{program} produced more than {} MiB of SVG",
            SVG_LIMIT >> 20
        )));
    }
    String::from_utf8(svg)
        .map_err(|error| failed(format!("{program} produced invalid UTF-8: {error}")))
}
