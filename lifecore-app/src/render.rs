use crate::{AppError, query::ViewQuery};
use lifecore::{Entry, Node, Payload};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    io::{self, Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
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

// Why `dot` failed after it started; `views.render` reports it as
// `graphviz_failed` beside the program's stderr.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphvizFailure {
    #[error("cannot start a thread for the pipes of dot: {message}")]
    PipeThread { message: String },
    #[error("cannot wait for dot: {message}")]
    Unwaitable { message: String },
    #[error("dot did not finish within {} and was killed", duration_words(*milliseconds))]
    Deadline { milliseconds: u64 },
    #[error("dot produced more than {} of SVG and was killed", size_words(*bytes))]
    Oversized { bytes: usize },
    // `status` is the platform's description, such as `exit status: 1`.
    #[error("dot exited with {status}")]
    Exited { status: String },
    #[error("cannot read the output of dot: {message}")]
    OutputUnreadable { message: String },
    #[error("the output of dot did not end within {} of its exit", duration_words(*milliseconds))]
    OutputOpen { milliseconds: u64 },
    // Without a message, writing was still blocked when the call gave up.
    #[error("dot did not read all of its input{}", message.as_ref().map(|message| format!(": {message}")).unwrap_or_default())]
    InputUnread { message: Option<String> },
    #[error("cannot read the stderr of dot: {message}")]
    StderrUnreadable { message: String },
    #[error("dot produced invalid UTF-8: {message}")]
    InvalidUtf8 { message: String },
}

fn duration_words(milliseconds: u64) -> String {
    if milliseconds.is_multiple_of(1000) {
        format!("{} s", milliseconds / 1000)
    } else {
        format!("{milliseconds} ms")
    }
}

fn size_words(bytes: usize) -> String {
    if bytes.is_multiple_of(1 << 20) {
        format!("{} MiB", bytes >> 20)
    } else {
        format!("{bytes} bytes")
    }
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

// How long `dot` may run, how much SVG it may write, and how long its pipe
// threads may take to report once it has exited or was killed. A descendant
// that inherited a pipe can hold it open indefinitely, so after `grace` the
// threads are abandoned.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) deadline: Duration,
    pub(crate) output: usize,
    pub(crate) grace: Duration,
}

impl Limits {
    pub(crate) const GRAPHVIZ: Self = Self {
        deadline: Duration::from_secs(10),
        output: 16 * 1024 * 1024,
        grace: Duration::from_millis(500),
    };
}

// The client's row label limit (INTERFACES 5.2), so a rendered node reads
// like its table row.
const LABEL_LIMIT: usize = 60;
const GRAPHVIZ: &str = "dot";
const GRAPHVIZ_POLL: Duration = Duration::from_millis(20);
// Enough of stderr to explain a failure without carrying a runaway log.
const STDERR_LIMIT: u64 = 64 * 1024;

pub(crate) fn render(query: &ViewQuery, format: RenderFormat) -> Result<Rendered, AppError> {
    let dot = to_dot(query);
    let text = match format {
    RenderFormat::Dot => dot,
    RenderFormat::Svg => graphviz(Command::new(GRAPHVIZ), &dot, Limits::GRAPHVIZ)?,
    };
    Ok(Rendered { format, text })
}

// The empty type name is legal and reads as on screen (INTERFACES 5.2).
fn type_name(name: &str) -> &str {
    if name.is_empty() { "(unnamed)" } else { name }
}

// The payload text a row is labeled by, as the client labels it (INTERFACES
// 5.2): `title`, then `name`, then the top-level text field whose name sorts
// first by bytes, which is the client's code point order; blank text does not
// count.
fn label_text(node: &Node) -> Option<&str> {
    let text = |field: &str| match node.payload.get(field) {
    Some(Payload::Text(text)) if !text.trim().is_empty() => Some(text.as_str()),
    _ => None,
    };
    text("title")
        .or_else(|| text("name"))
        .or_else(|| node.payload.keys().find_map(|field| text(field)))
}

// A table cell shows its text on one line, so each run of line breaks
// becomes a single space.
fn single_line(text: &str) -> String {
    let mut line = String::with_capacity(text.len());
    let mut breaking = false;
    for c in text.chars() {
        let line_break = matches!(c, '\n' | '\r');
        if !line_break {
            line.push(c);
        } else if !breaking {
            line.push(' ');
        }
        breaking = line_break;
    }
    line
}

// The full, uncut row label text.
fn row_text(node: &Node) -> String {
    match label_text(node) {
    Some(text) => single_line(text),
    None => format!("{} @{}", type_name(&node.ty_name), node.id.position()),
    }
}

// As the client cuts: text longer than the limit keeps one character less
// and an ellipsis, so the result never exceeds the limit.
fn cut(text: &str) -> String {
    let mut chars = text.chars();
    let kept = chars.by_ref().take(LABEL_LIMIT - 1).collect::<String>();
    match (chars.next(), chars.next()) {
    (None, _) => kept,
    (Some(last), None) => format!("{kept}{last}"),
    (Some(_), Some(_)) => format!("{kept}\u{2026}"),
    }
}

pub(crate) fn dot_quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
        // A doubled backslash also keeps Graphviz's own substitutions, such
        // as `\N`, literal.
        '\\' => quoted.push_str("\\\\"),
        '"' => quoted.push_str("\\\""),
        '\n' => quoted.push_str("\\n"),
        '\r' => quoted.push_str("\\r"),
        // Graphviz decodes HTML entities in labels before anything else, so
        // payload text spelling one must still read literally.
        '&' => quoted.push_str("&amp;"),
        // Graphviz has no escape for these: NUL breaks its parser, `\t`
        // reads as `t`, and the rest would reach SVG, where XML forbids or
        // discourages them.
        c if c.is_control() => quoted.push(' '),
        c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

// HACK: Graphviz (checked with 14.1) unescapes a tooltip twice, once as a
// tooltip and once while substituting `\N` and the like, so a backslash is
// doubled before the label quoting doubles it again.
fn tooltip_quote(text: &str) -> String {
    dot_quote(&text.replace('\\', "\\\\"))
}

// The queried subgraph: nodes in append order, then revision links between
// result nodes, then the result edges. Every node and edge carries its
// position as `id` and an inspector `href`, which Graphviz keeps in SVG
// output, so the picture is clickable; its tooltip is the label before
// cutting, since Graphviz would otherwise show the escaped label.
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
        let text = row_text(node);
        let identity = format!("@{} {}", node.id.position(), type_name(&node.ty_name));
        writeln!(
            dot,
            "  n{} [label={}, tooltip={}, id=\"n{}\", href=\"#inspect/{}/{}\"{}];",
            node.id.position(),
            dot_quote(&format!("{}\n{identity}", cut(&text))),
            tooltip_quote(&format!("{text}\n{identity}")),
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
        let label = format!("@{} {}", edge.id.position(), type_name(&edge.ty_name));
        // The label is a separate link in SVG, with a tooltip of its own.
        let tooltip = tooltip_quote(&label);
        writeln!(
            dot,
            "  n{} -> n{} [label={}, tooltip={tooltip}, labeltooltip={tooltip}, id=\"e{}\", href=\"#inspect/{}/{}\", style=solid{}];",
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

// What a pipe thread reports, once, when it is done.
enum Report {
    // All of stdout, or the failure reading it.
    Output(io::Result<Vec<u8>>),
    // Stdout passed the cap, and the reader stopped there.
    Oversized,
    // Stderr up to its limit, after draining the rest.
    Errors(io::Result<Vec<u8>>),
    // The whole input was written and the pipe closed.
    Input(io::Result<()>),
}

#[derive(Default)]
struct Pipes {
    output: Option<io::Result<Vec<u8>>>,
    oversized: bool,
    errors: Option<io::Result<Vec<u8>>>,
    input: Option<io::Result<()>>,
}

impl Pipes {
    fn record(&mut self, report: Report) {
        match report {
        Report::Output(output) => self.output = Some(output),
        Report::Oversized => self.oversized = true,
        Report::Errors(errors) => self.errors = Some(errors),
        Report::Input(input) => self.input = Some(input),
        }
    }
    fn settled(&self) -> bool {
        (self.output.is_some() || self.oversized) && self.errors.is_some() && self.input.is_some()
    }
}

enum Ending {
    Exited(ExitStatus),
    Oversized,
    TimedOut,
    Unwaitable(io::Error),
}

type Job = Box<dyn FnOnce() -> Report + Send>;

// Collects a killed child without waiting past `until`; one still running
// then is left behind rather than blocking the call.
fn reap(child: &mut Child, until: Instant) {
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
        thread::sleep(GRAPHVIZ_POLL);
    }
}

// Runs `command -Tsvg` synchronously with the DOT text on stdin, within
// `limits`. Each pipe has its own detached thread that reports over a
// channel, and none is ever joined: a descendant of the program can keep a
// pipe open after the program itself exits or is killed.
pub(crate) fn graphviz(
    mut command: Command,
    dot: &str,
    limits: Limits,
) -> Result<String, AppError> {
    // Whatever stops the program from starting, missing or not executable,
    // the operating system's text explains it.
    let mut child = command
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::GraphvizUnavailable {
            message: error.to_string(),
        })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let input = dot.to_owned();
    let cap = limits.output;
    let jobs: [Job; 3] = [
        Box::new(move || {
            let written = stdin.write_all(input.as_bytes());
            // Closing stdin ends the input.
            drop(stdin);
            Report::Input(written)
        }),
        // Stops at the cap rather than draining, so the loop below can kill
        // the program at once.
        Box::new(move || {
            let mut bytes = Vec::new();
            match stdout.take(cap as u64 + 1).read_to_end(&mut bytes) {
            Err(error) => Report::Output(Err(error)),
            Ok(_) if bytes.len() > cap => Report::Oversized,
            Ok(_) => Report::Output(Ok(bytes)),
            }
        }),
        // Keeps draining past its limit, so a verbose run is not stopped by
        // a full pipe.
        Box::new(move || {
            let mut bytes = Vec::new();
            let read = stderr
                .by_ref()
                .take(STDERR_LIMIT)
                .read_to_end(&mut bytes)
                .and_then(|_| io::copy(&mut stderr, &mut io::sink()));
            Report::Errors(read.map(|_| bytes))
        }),
    ];
    let (sender, receiver) = mpsc::channel();
    for job in jobs {
        let sender = sender.clone();
        // A report sent after the call gave up has no receiver and is dropped.
        let started = thread::Builder::new()
            .name("graphviz-pipe".into())
            .spawn(move || sender.send(job()).ok());
        if let Err(error) = started {
            child.kill().ok();
            reap(&mut child, Instant::now() + limits.grace);
            return Err(AppError::GraphvizFailed {
                cause: GraphvizFailure::PipeThread {
                    message: error.to_string(),
                },
                stderr: String::new(),
            });
        }
    }
    drop(sender);
    let mut pipes = Pipes::default();
    let deadline = Instant::now() + limits.deadline;
    let ending = loop {
        if pipes.oversized {
            break Ending::Oversized;
        }
        match child.try_wait() {
        Ok(Some(status)) => break Ending::Exited(status),
        Ok(None) => {}
        Err(error) => break Ending::Unwaitable(error),
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break Ending::TimedOut;
        }
        // Waiting on the channel instead of sleeping wakes the loop as soon
        // as stdout passes the cap.
        match receiver.recv_timeout(GRAPHVIZ_POLL.min(left)) {
        Ok(report) => pipes.record(report),
        Err(RecvTimeoutError::Timeout) => {}
        // Every thread has reported; only the exit is left to poll.
        Err(RecvTimeoutError::Disconnected) => thread::sleep(GRAPHVIZ_POLL.min(left)),
        }
    };
    let grace = Instant::now() + limits.grace;
    if !matches!(ending, Ending::Exited(_)) {
        child.kill().ok();
        reap(&mut child, grace);
    }
    while !pipes.settled() {
        let left = grace.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
        Ok(report) => pipes.record(report),
        Err(_) => break,
        }
    }
    // Threads that have not reported by now are abandoned with the receiver.
    drop(receiver);
    let stderr = match &pipes.errors {
    Some(Ok(bytes)) => String::from_utf8_lossy(bytes).trim().to_owned(),
    _ => String::new(),
    };
    outcome(ending, pipes, limits).map_err(|cause| AppError::GraphvizFailed { cause, stderr })
}

// The SVG, or the first reason the run cannot be trusted to have produced
// all of it.
fn outcome(ending: Ending, pipes: Pipes, limits: Limits) -> Result<String, GraphvizFailure> {
    let oversized = GraphvizFailure::Oversized {
        bytes: limits.output,
    };
    let status = match ending {
    Ending::Exited(status) => status,
    Ending::Oversized => return Err(oversized),
    Ending::TimedOut => {
        return Err(GraphvizFailure::Deadline {
            milliseconds: milliseconds(limits.deadline),
        });
    }
    Ending::Unwaitable(error) => {
        return Err(GraphvizFailure::Unwaitable {
            message: error.to_string(),
        });
    }
    };
    // Output can pass the cap just before the program exits.
    if pipes.oversized {
        return Err(oversized);
    }
    if !status.success() {
        return Err(GraphvizFailure::Exited {
            status: status.to_string(),
        });
    }
    let svg = match pipes.output {
    Some(Ok(svg)) => svg,
    Some(Err(error)) => {
        return Err(GraphvizFailure::OutputUnreadable {
            message: error.to_string(),
        });
    }
    None => {
        return Err(GraphvizFailure::OutputOpen {
            milliseconds: milliseconds(limits.grace),
        });
    }
    };
    // An exit before reading all input means the picture may be partial.
    match pipes.input {
    Some(Ok(())) => {}
    Some(Err(error)) => {
        return Err(GraphvizFailure::InputUnread {
            message: Some(error.to_string()),
        });
    }
    None => return Err(GraphvizFailure::InputUnread { message: None }),
    }
    if let Some(Err(error)) = pipes.errors {
        return Err(GraphvizFailure::StderrUnreadable {
            message: error.to_string(),
        });
    }
    String::from_utf8(svg).map_err(|error| GraphvizFailure::InvalidUtf8 {
        message: error.to_string(),
    })
}
