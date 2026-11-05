use crate::{
    DECLARATIONS, MIN_REVIEW_EVERY, REVIEW_EVERY_TASK, REVIEW_WEEKLY, Shape, declarations, history,
};
use lifecore::{
    Entry, EntryId, LifeCoreList,
    analysis::{AnalysisRunner, Config, EvaluationInputs, SuiteConfig},
    typing::Type,
};
use lifecore_reconnect::ReconnectFactory;
use lifecore_typecheck::TypeCheckFactory;

// The suite `lifecore init --default-analyzers` stores.
fn default_suite() -> SuiteConfig {
    SuiteConfig(
        [TypeCheckFactory::name(), ReconnectFactory::name()]
            .map(|name| Config {
                name: name.into(),
                configuration: "{}".into(),
            })
            .into(),
    )
}

fn generate(entries: usize, review_every: usize) -> LifeCoreList {
    history(
        Shape {
            entries,
            review_every,
        },
        default_suite(),
    )
}

fn entries(graph: &LifeCoreList) -> Vec<&Entry> {
    graph.view().entries().collect()
}

fn id(graph: &LifeCoreList, position: usize) -> EntryId {
    entries(graph)[position].id()
}

fn review_positions(graph: &LifeCoreList) -> Vec<usize> {
    graph
        .view()
        .entries()
        .filter(|entry| entry.as_node().is_some_and(|node| node.ty_name == "Review"))
        .map(|entry| entry.id().position())
        .collect()
}

#[test]
fn histories_validate_and_raise_no_finding_under_the_default_suite() {
    for review_every in [MIN_REVIEW_EVERY, REVIEW_EVERY_TASK, 40] {
        let graph = generate(150, review_every);
        assert_eq!(graph.view().len(), 150);
        assert_eq!(graph.analysis_suite(), &default_suite());
        graph.basic_validation().unwrap();
        let mut runner = AnalysisRunner::new(EvaluationInputs::default());
        runner
            .register_analyzer_kind(Box::new(TypeCheckFactory))
            .unwrap();
        runner
            .register_analyzer_kind(Box::new(ReconnectFactory))
            .unwrap();
        // Full replay, so every prefix of the history is judged.
        let output = runner.run(&graph).unwrap();
        let names = output
            .implementations
            .iter()
            .map(|identity| identity.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, [TypeCheckFactory::name(), ReconnectFactory::name()]);
        let observations = output.observations().collect::<Vec<_>>();
        assert!(observations.is_empty(), "{observations:#?}");
    }
}

#[test]
fn declarations_open_every_history() {
    let graph = generate(DECLARATIONS, REVIEW_EVERY_TASK);
    let declared = declarations();
    let names = declared
        .iter()
        .map(|ty| ty.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "Goal", "Concern", "Project", "Task", "Habit", "External", "Stance", "Done", "Did",
            "Review"
        ]
    );
    for (entry, ty) in entries(&graph).into_iter().zip(&declared) {
        let node = entry.as_node().unwrap();
        assert_eq!(node.ty_name, Type::TYPE_NAME);
        assert_eq!(node.predecessor, None);
        assert_eq!(node.payload, ty.to_body());
    }
    assert_eq!(graph.view().len(), DECLARATIONS);
}

// PERFORMANCE §1's windows: after a five-entry first window, a `Done` and its
// edge, the next task, its revision, its stance and edge, and the review.
#[test]
fn every_task_density_has_the_measured_window_shape() {
    let graph = generate(10 + 5 + 7 + 7, REVIEW_EVERY_TASK);
    let shape = entries(&graph)[DECLARATIONS..]
        .iter()
        .map(|entry| match entry {
        Entry::Node(node) => (
            node.ty_name.as_str(),
            node.predecessor.map(EntryId::position),
        ),
        Entry::Edge(edge) => (edge.ty_name.as_str(), None),
        })
        .collect::<Vec<_>>();
    let task = ("Task", None);
    let revision = |of| ("Task", Some(of));
    let (stance, about, review, done) = (
        ("Stance", None),
        ("About", None),
        ("Review", None),
        ("Done", None),
    );
    assert_eq!(
        shape,
        [
            task,
            revision(10),
            stance,
            about,
            review,
            done,
            about,
            task,
            revision(17),
            stance,
            about,
            review,
            done,
            about,
            task,
            revision(24),
            stance,
            about,
            review,
        ]
    );
    // Both edges of a task leave its stance and its `Done` for its revision.
    for (edge, from, to) in [(13, 12, 11), (16, 15, 11), (20, 19, 18), (23, 22, 18)] {
        let edge = entries(&graph)[edge].as_edge().unwrap();
        assert_eq!((edge.from, edge.to), (id(&graph, from), id(&graph, to)));
    }
    // One origin per window, the declarations included.
    let origins = entries(&graph)
        .iter()
        .map(|entry| entry.id().origin())
        .collect::<Vec<_>>();
    for window in [0..10, 10..15, 15..22, 22..29] {
        assert!(
            origins[window.clone()]
                .iter()
                .all(|o| *o == origins[window.start])
        );
    }
    assert_eq!(
        origins
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
}

#[test]
fn reviews_close_each_window_and_match_the_shape_count() {
    for (entries, review_every, positions) in [
        (DECLARATIONS, REVIEW_EVERY_TASK, vec![]),
        (14, REVIEW_EVERY_TASK, vec![]),
        (15, REVIEW_EVERY_TASK, vec![14]),
        (29, REVIEW_EVERY_TASK, vec![14, 21, 28]),
        (13, MIN_REVIEW_EVERY, vec![10]),
        (1_007, REVIEW_WEEKLY, vec![]),
        (1_008, REVIEW_WEEKLY, vec![1_007]),
        (2_100, REVIEW_WEEKLY, vec![1_007, 2_007]),
    ] {
        let shape = Shape {
            entries,
            review_every,
        };
        let graph = history(shape, default_suite());
        assert_eq!(review_positions(&graph), positions, "{shape:?}");
        assert_eq!(shape.reviews(), positions.len(), "{shape:?}");
    }
}

#[test]
fn generation_is_deterministic_and_prefix_stable() {
    for review_every in [REVIEW_EVERY_TASK, REVIEW_WEEKLY] {
        let long = generate(1_100, review_every);
        assert_eq!(long, generate(1_100, review_every));
        for boundary in [DECLARATIONS, 14, 15, 500, 1_007, 1_008] {
            let short = generate(boundary, review_every);
            let prefix = long.view().prefix(boundary).unwrap();
            assert_eq!(entries(&short), prefix.entries().collect::<Vec<_>>());
            assert_eq!(short.view().id().database, long.view().id().database);
        }
    }
}
