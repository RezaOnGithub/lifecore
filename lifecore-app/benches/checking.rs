//! The checking benchmark of PERFORMANCE §1. For planning-shaped histories
//! from `lifecore-workload` at both seal densities, it times loading and
//! validating the snapshot (`database.describe`), and checking a one-node
//! proposal under the default suite, which loads the snapshot again and
//! replays every prefix. Each cell is the median of several runs.
//!
//! Run it with `cargo bench -p lifecore-app`. `LIFECORE_BENCH_SIZES` takes
//! comma-separated entry counts (default `500,1000,2000,4000`), and
//! `LIFECORE_BENCH_RUNS` the runs per cell (default 3).

use lifecore::{Body, Payload, analysis::AnalyzerFactory};
use lifecore_app::{
    Service,
    proposal::{Proposal, ProposalEntry},
    service::{NoParams, ProposalRequest},
    storage::{Snapshot, StatePath},
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_reconnect::ReconnectFactory;
use lifecore_typecheck::TypeCheckFactory;
use lifecore_workload::{REVIEW_EVERY_TASK, REVIEW_WEEKLY, Shape, default_suite, history};
use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    time::{Duration, Instant},
};

const SIZES: &str = "LIFECORE_BENCH_SIZES";
const RUNS: &str = "LIFECORE_BENCH_RUNS";
const DEFAULT_SIZES: [usize; 4] = [500, 1_000, 2_000, 4_000];
const DEFAULT_RUNS: usize = 3;
const DENSITIES: [usize; 2] = [REVIEW_EVERY_TASK, REVIEW_WEEKLY];

// The `lifecore` binary's factories, from `lifecore-cli/src/main.rs`, which
// lives in a binary crate. A factory missing here fails the check of a suite
// that names it; the suite itself is `lifecore_workload::default_suite`.
fn factories() -> Vec<Box<dyn AnalyzerFactory>> {
    vec![
        Box::new(TypeCheckFactory),
        Box::new(DagCheckFactory),
        Box::new(ReconnectFactory),
    ]
}

fn setting<T: std::str::FromStr>(name: &str, text: &str) -> T {
    text.trim()
        .parse()
        .unwrap_or_else(|_| panic!("{name}: {text:?} is not a whole number"))
}

fn sizes() -> Vec<usize> {
    let Ok(text) = env::var(SIZES) else {
        return DEFAULT_SIZES.into();
    };
    text.split(',').map(|size| setting(SIZES, size)).collect()
}

fn runs() -> usize {
    let runs = env::var(RUNS).map_or(DEFAULT_RUNS, |text| setting(RUNS, &text));
    assert!(runs > 0, "{RUNS} must be positive");
    runs
}

fn one_node() -> ProposalRequest {
    ProposalRequest {
        proposal: Proposal {
            plan_id: None,
            acknowledges: None,
            entries: vec![ProposalEntry::Node {
                name: "task".into(),
                ty_name: "Task".into(),
                predecessor: None,
                intent: None,
                payload: Body::from([("title".into(), Payload::Text("Benchmark".into()))]),
            }],
        },
        evaluation_time: None,
        now: false,
    }
}

// Written as `storage` publishes a snapshot, pretty-printed, so load times
// parse the same text a database holds.
fn write_snapshot(path: &Path, shape: Shape) {
    let snapshot = Snapshot::new(history(shape, default_suite()));
    let mut bytes = serde_json::to_vec_pretty(&snapshot).expect("snapshot serializes");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("snapshot is written");
}

fn median(runs: usize, mut run: impl FnMut()) -> Duration {
    let mut times = (0..runs)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed()
        })
        .collect::<Vec<_>>();
    times.sort();
    // The mean of the middle two for an even number of runs.
    (times[(runs - 1) / 2] + times[runs / 2]) / 2
}

// Two significant digits, as PERFORMANCE records them.
fn seconds(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    let magnitude = if seconds > 0.0 {
        seconds.log10().floor() as i32
    } else {
        0
    };
    let decimals = (1 - magnitude).max(0) as usize;
    format!("{seconds:.decimals$} s")
}

fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

fn main() {
    // `cargo bench` passes `--bench`; `cargo test --benches` or
    // `--all-targets` runs this target without it, in a debug build, where
    // the full benchmark would take minutes.
    if !env::args().any(|argument| argument == "--bench") {
        return;
    }
    let (sizes, runs) = (sizes(), runs());
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("checking");
    fs::create_dir_all(&directory).expect("benchmark directory");
    let state = StatePath::new(directory.join("LifeCoreState.json"));
    let service = Service::new(state.clone(), Box::new(factories));

    println!("Median of {runs} runs; one review per 7 entries, then per 1,000.\n");
    let mut header = vec!["Entries".to_owned()];
    for density in DENSITIES {
        let density = grouped(density);
        header.extend([
            format!("Reviews (1/{density})"),
            format!("load (1/{density})"),
            format!("check (1/{density})"),
        ]);
    }
    println!("| {} |", header.join(" | "));
    println!("| {} |", vec!["---:"; header.len()].join(" | "));
    for entries in sizes {
        let mut row = vec![grouped(entries)];
        for review_every in DENSITIES {
            let shape = Shape {
                entries,
                review_every,
            };
            write_snapshot(state.path(), shape);
            let load = median(runs, || {
                service.describe(NoParams {}).expect("snapshot loads");
            });
            let check = median(runs, || {
                let checked = service.check(one_node()).expect("proposal is checked");
                assert!(checked.accepted, "the one-node proposal is accepted");
                assert_eq!(checked.report.analysis.observations().count(), 0);
            });
            row.extend([grouped(shape.reviews()), seconds(load), seconds(check)]);
        }
        println!("| {} |", row.join(" | "));
        io::stdout().flush().ok();
    }
    fs::remove_file(state.path()).ok();
}
