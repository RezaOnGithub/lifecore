use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use lifecore::{LifeCoreAccess, LifeCoreFamily, LifeCoreList, Node};
use tempfile::NamedTempFile;

use crate::{
    error::AppError,
    locator::{EntryExpectation, ResolvedEntry, resolve_index},
    state::CliStateV1,
    view::GraphView,
};

const GRAPHVIZ_FINGERPRINT_LENGTH: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphvizOptions {
    pub full: bool,
    pub fields: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DotDocument(String);

impl DotDocument {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphvizKind {
    Dot,
    Svg,
}

impl GraphvizKind {
    pub fn name(self) -> &'static str {
        match self {
        Self::Dot => "DOT",
        Self::Svg => "SVG",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphvizTarget {
    path: PathBuf,
    kind: GraphvizKind,
}

impl GraphvizTarget {
    pub fn resolve(
        requested: PathBuf,
        current_directory: &Path,
        kind: GraphvizKind,
    ) -> Result<Self, AppError> {
        if requested.as_os_str().is_empty() {
            return Err(AppError::graphviz_target("artifact path is empty"));
        }

        let path = if requested.is_absolute() {
            requested
        } else {
            current_directory.join(requested)
        };

        Ok(Self { path, kind })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn kind(&self) -> GraphvizKind {
        self.kind
    }

    pub(crate) fn reject_protected_path(&self, protected: &Path) -> Result<(), AppError> {
        let protected = std::fs::canonicalize(protected).map_err(|error| {
            AppError::graphviz_target_io(protected, "resolve canonical state path", error)
        })?;
        let resolved_target = match std::fs::canonicalize(&self.path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = self.path.parent().ok_or_else(|| {
                AppError::graphviz_target("artifact path does not have a parent directory")
            })?;
            let file_name = self.path.file_name().ok_or_else(|| {
                AppError::graphviz_target("artifact path does not have a file name")
            })?;
            std::fs::canonicalize(parent)
                .map(|parent| parent.join(file_name))
                .unwrap_or_else(|_| self.path.clone())
        }
        Err(error) => {
            return Err(AppError::graphviz_target_io(
                &self.path,
                "resolve artifact path",
                error,
            ));
        }
        };

        if resolved_target == protected {
            return Err(AppError::graphviz_target(
                "artifact target resolves to the canonical state path",
            ));
        }

        Ok(())
    }
}

pub struct GraphvizInput<'a> {
    pub graph: &'a LifeCoreList,
    pub state: &'a CliStateV1,
    pub view: &'a GraphView,
    pub aliases: &'a BTreeMap<usize, Vec<String>>,
    pub canonical_end: usize,
}

#[derive(Clone, Debug)]
pub struct GraphvizExporter {
    dot_program: OsString,
}

impl GraphvizExporter {
    pub fn new(dot_program: impl Into<OsString>) -> Self {
        Self {
            dot_program: dot_program.into(),
        }
    }

    pub fn write_dot(
        &self,
        target: &GraphvizTarget,
        document: &DotDocument,
    ) -> Result<(), AppError> {
        let parent = target.path().parent().ok_or_else(|| {
            AppError::graphviz_target("artifact path does not have a parent directory")
        })?;
        let mut artifact = NamedTempFile::new_in(parent).map_err(|error| {
            AppError::graphviz_target_io(target.path(), "create temporary DOT", error)
        })?;
        artifact
            .write_all(document.as_bytes())
            .map_err(|error| AppError::graphviz_target_io(target.path(), "write DOT", error))?;
        artifact
            .flush()
            .map_err(|error| AppError::graphviz_target_io(target.path(), "flush DOT", error))?;
        artifact.persist(target.path()).map_err(|error| {
            AppError::graphviz_target_io(target.path(), "persist DOT", error.error)
        })?;

        Ok(())
    }

    pub fn write_svg(
        &self,
        target: &GraphvizTarget,
        document: &DotDocument,
    ) -> Result<(), AppError> {
        let parent = target.path().parent().ok_or_else(|| {
            AppError::graphviz_target("artifact path does not have a parent directory")
        })?;
        let mut artifact = NamedTempFile::new_in(parent).map_err(|error| {
            AppError::graphviz_target_io(target.path(), "create temporary SVG", error)
        })?;
        let stdout = artifact.reopen().map_err(|error| {
            AppError::graphviz_target_io(target.path(), "prepare SVG output", error)
        })?;
        let mut child = Command::new(&self.dot_program)
            .arg("-Tsvg")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| AppError::graphviz_process_spawn(&self.dot_program, error))?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            AppError::graphviz_process("dot did not provide the requested standard input")
        })?;

        //A Close dot's input before waiting so it observes the complete
        //A document, while its SVG output streams to a recoverable target-side file.
        if let Err(error) = stdin.write_all(document.as_bytes()) {
            drop(child.kill());
            drop(child.wait());

            return Err(AppError::graphviz_process_io("write dot stdin", error));
        }
        drop(stdin);

        let output = child
            .wait_with_output()
            .map_err(|error| AppError::graphviz_process_io("wait for dot", error))?;
        if !output.status.success() {
            return Err(AppError::graphviz_process_failed(
                &self.dot_program,
                output.status,
                &output.stderr,
            ));
        }

        artifact.flush().map_err(|error| {
            AppError::graphviz_target_io(target.path(), "flush SVG output", error)
        })?;
        artifact.persist(target.path()).map_err(|error| {
            AppError::graphviz_target_io(target.path(), "persist SVG output", error.error)
        })?;

        Ok(())
    }
}

impl Default for GraphvizExporter {
    fn default() -> Self {
        Self::new(OsStr::new("dot"))
    }
}

pub fn generate(
    input: GraphvizInput<'_>,
    options: GraphvizOptions,
) -> Result<DotDocument, AppError> {
    let nodes = input.graph.nodes(input.view.context()).collect::<Vec<_>>();
    let mut selected_nodes = BTreeMap::new();
    for node in &nodes {
        let stale = input
            .graph
            .is_stale(node.e)
            .ok_or_else(|| AppError::graphviz_generation("node freshness could not be resolved"))?;
        if options.full || !stale {
            //A Entry index is both the stable DOT identifier and the map key,
            //A making traversal independent of hash or allocation ordering.
            selected_nodes.insert(node.e.index().0, node);
        }
    }
    let selected_indices = selected_nodes.keys().copied().collect::<BTreeSet<_>>();
    let mut dot = String::from(
        "digraph lifecore {\n  graph [compound=true, rankdir=LR];\n  node [shape=Mrecord];\n",
    );

    //A Full mode owns family grouping; default mode keeps the smaller fresh
    //A graph flat so the two views remain visually and byte-wise distinct.
    if options.full {
        for family in input.graph.families(input.view.context()) {
            let mut members = Vec::new();
            collect_family_members(&family, &mut members);
            let Some(root) = members.first().copied() else {
                continue;
            };
            let root_locator = resolve_index(
                input.graph,
                input.state,
                root,
                EntryExpectation::Node,
                Some(input.view.end_exclusive()),
            )?;

            dot.push_str(&format!("  subgraph cluster_family_{root} {{\n"));
            dot.push_str(&format!(
                "    label=\"Family {}\";\n",
                escape_dot_string(&graphviz_locator(&root_locator))
            ));
            dot.push_str("    color=\"gray70\";\n");
            for index in members {
                let node = selected_nodes.get(&index).ok_or_else(|| {
                    AppError::graphviz_generation(format!(
                        "family references unavailable node {index}"
                    ))
                })?;
                write_node(&mut dot, &input, node, options, 4)?;
            }
            dot.push_str("  }\n");
        }
    } else {
        for node in selected_nodes.values() {
            write_node(&mut dot, &input, node, options, 2)?;
        }
    }

    if options.full {
        for node in &nodes {
            if let Some(parent) = node.parent {
                dot.push_str(&format!(
                    "  n{} -> n{} [arrowhead=none, label=\"parent\", style=dashed];\n",
                    parent.0,
                    node.e.index().0
                ));
            }
        }
    }

    for edge in input.graph.edges(input.view.context()) {
        let fresh = !input
            .graph
            .is_stale(edge.e)
            .ok_or_else(|| AppError::graphviz_generation("edge freshness could not be resolved"))?;
        if !options.full && !fresh {
            continue;
        }
        if !selected_indices.contains(&edge.from.0) || !selected_indices.contains(&edge.to.0) {
            return Err(AppError::graphviz_generation(format!(
                "visible edge {} has an unavailable endpoint",
                edge.e.index().0
            )));
        }
        let locator = resolve_index(
            input.graph,
            input.state,
            edge.e.index().0,
            EntryExpectation::Any,
            Some(input.view.end_exclusive()),
        )?;
        let provisional = edge.e.index().0 >= input.canonical_end;
        let label = if provisional {
            format!(
                "{}\n{}\nprovisional",
                edge.ty_name,
                graphviz_locator(&locator)
            )
        } else {
            format!("{}\n{}", edge.ty_name, graphviz_locator(&locator))
        };
        let provisional_style = if provisional {
            ", color=\"dodgerblue3\", fontcolor=\"dodgerblue4\", style=dashed"
        } else {
            ""
        };

        dot.push_str(&format!(
            "  n{} -> n{} [label=\"{}\"{}];\n",
            edge.from.0,
            edge.to.0,
            escape_dot_string(&label),
            provisional_style
        ));
    }

    dot.push_str("}\n");

    Ok(DotDocument(dot))
}

fn write_node(
    dot: &mut String,
    input: &GraphvizInput<'_>,
    node: &Node,
    options: GraphvizOptions,
    indentation: usize,
) -> Result<(), AppError> {
    let locator = resolve_index(
        input.graph,
        input.state,
        node.e.index().0,
        EntryExpectation::Node,
        Some(input.view.end_exclusive()),
    )?;
    let mut header_parts = vec![
        escape_record_component(&graphviz_locator(&locator)),
        escape_record_component(&node.ty_name),
    ];
    if let Some(names) = input.aliases.get(&node.e.index().0) {
        header_parts.push(escape_record_component(&format!(
            "aliases: {}",
            names.join(", ")
        )));
    }
    let provisional = node.e.index().0 >= input.canonical_end;
    if provisional {
        header_parts.push("provisional".to_string());
    }
    let header = format!("{{{}}}", header_parts.join("|"));
    let label = if options.fields {
        let mut rows = vec![header];
        for (key, value) in &node.field.0 {
            let value = serde_json::to_string(value).map_err(|error| {
                AppError::graphviz_generation(format!(
                    "node field {key:?} could not be encoded: {error}"
                ))
            })?;
            rows.push(format!(
                "{{{}|{}}}",
                escape_record_component(key),
                escape_record_component(&value)
            ));
        }

        //A The extra nested record flips orientation: the header and each
        //A field become vertical rows, while every row remains key-value.
        format!("{{{{{}}}}}", rows.join("|"))
    } else {
        header
    };

    let provisional_style = if provisional {
        ", color=\"dodgerblue3\", fontcolor=\"dodgerblue4\", style=\"dashed,bold\""
    } else {
        ""
    };
    dot.push_str(&format!(
        "{:indentation$}n{} [label=\"{}\"{}];\n",
        "",
        node.e.index().0,
        label,
        provisional_style,
        indentation = indentation
    ));

    Ok(())
}

fn graphviz_locator(entry: &ResolvedEntry) -> String {
    format!(
        "{}:{}",
        entry.index(),
        &entry.fingerprint.as_str()[..GRAPHVIZ_FINGERPRINT_LENGTH]
    )
}

fn collect_family_members(family: &LifeCoreFamily, members: &mut Vec<usize>) {
    match family {
    LifeCoreFamily::Leaf(entry) => members.push(entry.index().0),
    LifeCoreFamily::Parent(entry, children) => {
        members.push(entry.index().0);
        for child in children {
            collect_family_members(child, members);
        }
    }
    }
}

fn escape_dot_string(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
        '\\' => escaped.push_str("\\\\"),
        '"' => escaped.push_str("\\\""),
        '\n' => escaped.push_str("\\n"),
        '\r' => escaped.push_str("\\r"),
        '\t' => escaped.push_str("\\t"),
        character if character.is_control() => {
            escaped.push_str(&format!("\\u{:04x}", u32::from(character)));
        }
        character => escaped.push(character),
        }
    }

    escaped
}

fn escape_record_component(value: &str) -> String {
    let mut escaped = String::new();

    //A Record delimiters need escaping in addition to ordinary quoted-DOT
    //A characters, otherwise stored data could change the label structure.
    for character in value.chars() {
        match character {
        '\\' => escaped.push_str("\\\\"),
        '"' => escaped.push_str("\\\""),
        '{' => escaped.push_str("\\{"),
        '}' => escaped.push_str("\\}"),
        '|' => escaped.push_str("\\|"),
        '<' => escaped.push_str("\\<"),
        '>' => escaped.push_str("\\>"),
        '\n' => escaped.push_str("\\n"),
        '\r' => escaped.push_str("\\r"),
        '\t' => escaped.push_str("\\t"),
        character if character.is_control() => {
            escaped.push_str(&format!("\\u{:04x}", u32::from(character)));
        }
        character => escaped.push(character),
        }
    }

    escaped
}
