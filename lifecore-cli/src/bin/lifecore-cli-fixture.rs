use std::{env, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use lifecore::{Body, LifeCore, LifeCoreList, OrderedSet, Payload};
use lifecore_cli::{
    analysis::initial_graph,
    locator::{EntryExpectation, resolve_index},
    reference::{NodeReference, resolve_node_reference},
    state::{PersistedState, StatePath, StateStore},
    view::GraphView,
};
use serde_json::json;

#[derive(Parser)]
#[command(name = "lifecore-cli-fixture")]
struct FixtureCli {
    #[command(subcommand)]
    command: FixtureCommand,
}

#[derive(Subcommand)]
enum FixtureCommand {
    Stage3(Stage3Args),
    Stage7(Stage7Args),
}

#[derive(Args)]
struct Stage3Args {
    #[arg(long, default_value = "Stage3LifeCoreState.json")]
    path: PathBuf,
}

#[derive(Args)]
struct Stage7Args {
    #[arg(long, default_value = "Stage7LifeCoreState.json")]
    path: PathBuf,
}

fn main() -> ExitCode {
    match run(FixtureCli::parse()) {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    }
    }
}

fn run(cli: FixtureCli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
    FixtureCommand::Stage3(args) => build_stage3(args),
    FixtureCommand::Stage7(args) => build_stage7(args),
    }
}

fn build_stage7(args: Stage7Args) -> Result<(), Box<dyn std::error::Error>> {
    let current_directory = env::current_dir()?;
    let path = StatePath::resolve(Some(args.path.into_os_string()), &current_directory)?;
    let store = StateStore::new(path);
    //A Stage 7 walkthroughs need successful child transactions without also
    //A taking ownership of the deferred type-editor bootstrap workflow.
    let mut graph = LifeCoreList::default();
    let original = graph.add_mock("Note", Body::default(), None)?;
    let task = graph.add_mock("Task", Body::default(), None)?;
    let revised = graph.add_mock("Note", Body::default(), Some(original.e))?;
    let edge = graph.connect_mock(revised.e, "Relates_To", task.e)?;

    graph.apply()?;
    let state = PersistedState::new(graph);
    store.initialize(&state)?;

    let view = GraphView::latest(&state.lifecore, &state.cli)?;
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        original.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let task = resolve_index(
        &state.lifecore,
        &state.cli,
        task.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let revised = resolve_index(
        &state.lifecore,
        &state.cli,
        revised.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let edge = resolve_index(
        &state.lifecore,
        &state.cli,
        edge.e.entry().0,
        EntryExpectation::Any,
        Some(view.end_exclusive()),
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scenario": "stage7",
            "state_path": store.path(),
            "view": view.summary(),
            "locators": {
                "original_note": original.full_locator(),
                "task": task.full_locator(),
                "revised_note": revised.full_locator(),
                "final_edge": edge.full_locator(),
                "historical_boundary": task.full_locator(),
            }
        }))?
    );

    Ok(())
}

fn build_stage3(args: Stage3Args) -> Result<(), Box<dyn std::error::Error>> {
    let current_directory = env::current_dir()?;
    let path = StatePath::resolve(Some(args.path.into_os_string()), &current_directory)?;
    let store = StateStore::new(path);
    let mut graph = initial_graph();

    let original = graph.add_mock(
        "Note",
        Body::from([
            ("title", Payload::Text("Draft note".to_string())),
            ("score", Payload::Number(1)),
            (
                "history",
                Payload::Sequence(vec![Payload::Text("draft".to_string()), Payload::Number(1)]),
            ),
            (
                "tags",
                Payload::OrderedSet(OrderedSet::from([
                    Payload::Text("archived".to_string()),
                    Payload::Text("review".to_string()),
                ])),
            ),
        ]),
        None,
    )?;
    let task = graph.add_mock(
        "Task",
        Body::from([
            ("title", Payload::Text("Pay invoice".to_string())),
            ("score", Payload::Number(2)),
            (
                "history",
                Payload::Sequence(vec![Payload::Text("open".to_string())]),
            ),
            (
                "tags",
                Payload::OrderedSet(OrderedSet::from([Payload::Text("urgent".to_string())])),
            ),
        ]),
        None,
    )?;
    let revised = graph.add_mock(
        "Note",
        Body::from([
            ("title", Payload::Text("Published note".to_string())),
            ("score", Payload::Number(3)),
            (
                "history",
                Payload::Sequence(vec![
                    Payload::Text("draft".to_string()),
                    Payload::Text("published".to_string()),
                ]),
            ),
            (
                "tags",
                Payload::OrderedSet(OrderedSet::from([Payload::Text("review".to_string())])),
            ),
        ]),
        Some(original.e),
    )?;
    let edge = graph.connect_mock(revised.e, "Relates_To", task.e)?;

    //A Applying once makes the entire provisional suffix canonical without
    //A changing the Stage 3 fingerprints, which intentionally omit circumstance.
    graph.apply()?;
    let state = PersistedState::new(graph);
    store.initialize(&state)?;

    let view = GraphView::latest(&state.lifecore, &state.cli)?;
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        original.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let task = resolve_index(
        &state.lifecore,
        &state.cli,
        task.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let revised = resolve_index(
        &state.lifecore,
        &state.cli,
        revised.e.entry().0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )?;
    let edge = resolve_index(
        &state.lifecore,
        &state.cli,
        edge.e.entry().0,
        EntryExpectation::Any,
        Some(view.end_exclusive()),
    )?;
    let head = resolve_node_reference(&state.lifecore, &state.cli, &NodeReference::Head, &view)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scenario": "stage3",
            "state_path": store.path(),
            "view": view.summary(),
            "locators": {
                "original_note": original.full_locator(),
                "task": task.full_locator(),
                "revised_note": revised.full_locator(),
                "final_edge": edge.full_locator(),
                "historical_boundary": task.full_locator(),
                "head": head.full_locator(),
            }
        }))?
    );

    Ok(())
}
