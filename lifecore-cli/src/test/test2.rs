use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::Parser;
use lifecore::{
    LifeCore, LifeCoreList, Payload,
    serialize::{LifeCoreSerializeExt, NodeBody},
    typing::Type,
};

use crate::{
    Cli,
    interactive_analyzer::InteractiveFactory,
    locator::{self, Locator},
    payload_input,
    state::StateFile,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lifecore-cli-test-{}-{}-{name}.json",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn initialized(path: &std::path::Path) -> StateFile {
    let mut state = StateFile::new();
    state
        .lifecore
        .add_analyzer(InteractiveFactory::<LifeCoreList>::name(), "");
    state.lifecore.add_analyzer("Type_Check", "");
    state
        .lifecore
        .add_typed_mock(Type::describe_self(), None)
        .unwrap();
    state.lifecore.apply().unwrap();
    state.create(path).unwrap();
    state
}

#[test]
fn versioned_state_round_trips_and_rejects_old_shape() {
    let state_path = path("state");
    let state = StateFile::new();
    state.create(&state_path).unwrap();
    let loaded = StateFile::load(&state_path).unwrap();

    assert_eq!(loaded.cli.state_format_version, 1);
    assert_eq!(loaded.cli.state_uuid.get_version_num(), 7);

    fs::write(&state_path, r#"{"lifecore":{},"cli":{}}"#).unwrap();
    assert!(StateFile::load(&state_path).is_err());
    fs::remove_file(state_path).unwrap();
}

#[test]
fn locator_rejects_a_correct_index_with_the_wrong_digest() {
    let state_path = path("locator");
    let state = initialized(&state_path);
    let view = crate::state::latest_view(&state.lifecore);
    let wrong: Locator = "0:00000".parse().unwrap();

    let error = locator::resolve(state.cli.state_uuid, &state.lifecore, &wrong, view)
        .unwrap_err()
        .to_string();

    assert!(error.contains("fingerprint mismatch"));
    fs::remove_file(state_path).unwrap();
}

#[test]
fn recursive_payload_input_rejects_duplicate_ordered_set_elements() {
    let body = payload_input::assignments(&[
        "nested = { sequence = [{ ordered_set = [1, 2] }, \"ok\"] }".to_string(),
    ])
    .unwrap();
    assert!(matches!(body.get("nested"), Some(Payload::Sequence(_))));

    assert!(
        payload_input::assignments(&["bad = { ordered_set = [\"same\", \"same\"] }".to_string()])
            .is_err()
    );
}

#[test]
fn dry_run_leaves_state_bytes_unchanged() {
    let state_path = path("dry-run");
    initialized(&state_path);
    let before = fs::read(&state_path).unwrap();
    let cli = Cli::try_parse_from([
        "lifecore-cli",
        "type",
        "create",
        "Task",
        "--field",
        "title = Text : Mandatory",
    ])
    .unwrap();

    cli.execute_at(&state_path).unwrap();

    assert_eq!(fs::read(&state_path).unwrap(), before);
    fs::remove_file(state_path).unwrap();
}

#[test]
fn head_guard_mismatch_saves_nothing() {
    let state_path = path("head");
    initialized(&state_path);
    let before = fs::read(&state_path).unwrap();
    let cli = Cli::try_parse_from([
        "lifecore-cli",
        "type",
        "create",
        "Task",
        "--commit",
        "--expect-head",
        "0:00000",
    ])
    .unwrap();

    assert!(cli.execute_at(&state_path).is_err());
    assert_eq!(fs::read(&state_path).unwrap(), before);
    fs::remove_file(state_path).unwrap();
}
