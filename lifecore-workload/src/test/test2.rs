use crate::{
    DECLARATIONS, MIN_REVIEW_EVERY, REVIEW_EVERY_TASK, REVIEW_WEEKLY, Shape, declarations,
    default_suite, history, varied_history,
};
use lifecore::{
    Entry, EntryId, LifeCoreList, View,
    analysis::{
        AnalysisRunner, Config, EvaluationInputs, SuiteConfig, assert_indexing_equivalence,
    },
    typing::Type,
};
use lifecore_app::proposal::{self, Format, ProposalEntry};
use lifecore_dag_check::DagCheckFactory;
use lifecore_reconnect::ReconnectFactory;
use lifecore_typecheck::TypeCheckFactory;
use std::collections::{BTreeMap, BTreeSet};

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

// The three declarations PLANNING §11 prints at `59c702c`, verbatim.
const PLANNING_DECLARATIONS: &str = r#"
[[entries]]
name = "task_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], note = ["optional", "text"] } }
outgoing = ["Enables", "Helps"]
incoming = ["Enables", "Helps", "About", "Until"]
successors = []

[[entries]]
name = "stance_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Stance"
fields = { kind = "record", value = { kind = ["mandatory", "text"], at = ["mandatory", "timestamp"], cost = ["optional", "integer"], horizon = ["optional", "integer"], reason = ["optional", "text"] } }
outgoing = ["About", "Until"]
incoming = []
successors = []

[[entries]]
name = "review_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Review"
fields = { kind = "record", value = { budget = ["mandatory", "integer"], at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = []
incoming = []
successors = []
"#;

#[test]
fn declarations_match_the_ones_planning_printed() {
    let printed = proposal::decode(PLANNING_DECLARATIONS, Format::Toml).unwrap();
    let declared = declarations();
    assert_eq!(printed.entries.len(), 3);
    for entry in printed.entries {
        let ProposalEntry::Node {
            ty_name, payload, ..
        } = entry
        else {
            panic!("a declaration is a node");
        };
        assert_eq!(ty_name, Type::TYPE_NAME);
        let ty = Type::from_body(&payload).unwrap();
        let generated = declared.iter().find(|candidate| candidate.name == ty.name);
        // Compared as declarations: permissions are sets, which `to_body`
        // writes sorted rather than in the printed order.
        assert_eq!(generated, Some(&ty));
    }
}

// PERFORMANCE §1.2's windows: after a five-entry first window, a `Done` and its
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

fn varied(entries: usize, review_every: usize, seed: u64) -> LifeCoreList {
    varied_history(
        Shape {
            entries,
            review_every,
        },
        seed,
        default_suite(),
    )
}

#[test]
fn varied_histories_are_deterministic_prefix_stable_and_start_like_the_plain_ones() {
    let plain = generate(400, REVIEW_EVERY_TASK);
    for seed in [1, 2] {
        let long = varied(400, REVIEW_EVERY_TASK, seed);
        assert_eq!(long, varied(400, REVIEW_EVERY_TASK, seed));
        assert_ne!(long, varied(400, REVIEW_EVERY_TASK, seed + 10));
        assert_ne!(entries(&long), entries(&plain));
        assert_eq!(
            entries(&long)[..DECLARATIONS],
            entries(&plain)[..DECLARATIONS]
        );
        assert_eq!(long.analysis_suite(), &default_suite());
        for boundary in [DECLARATIONS, 11, 57, 200, 399] {
            let short = varied(boundary, REVIEW_EVERY_TASK, seed);
            let prefix = long.view().prefix(boundary).unwrap();
            assert_eq!(entries(&short), prefix.entries().collect::<Vec<_>>());
        }
    }
}

// What a varied history holds that the plain one does not, by the variation
// an index could get wrong.
#[test]
fn varied_histories_hold_the_variation_an_index_could_get_wrong() {
    let graph = varied(600, REVIEW_EVERY_TASK, 3);
    let view = graph.view();
    let declarations = view.nodes_of_type(Type::TYPE_NAME).collect::<Vec<_>>();
    assert!(declarations.iter().any(|node| node.predecessor.is_some()));
    assert!(
        declarations
            .iter()
            .any(|node| Type::from_body(&node.payload).is_err())
    );
    assert!(
        (0..=view.len()).any(|end| {
            let prefix = view.prefix(end).unwrap();
            let names = prefix.declarations();
            names.names().any(|name| names.claimants(name).count() > 1)
        }),
        "a name is claimed twice at some boundary"
    );
    // Revision chains longer than one, and splits.
    assert!(
        view.nodes()
            .any(|node| view.ancestors(node.id).count() >= 2 && node.ty_name != Type::TYPE_NAME)
    );
    assert!(view.nodes().any(|node| view.children(node.id).count() >= 2));
    let labels = view
        .edges()
        .map(|edge| edge.ty_name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        labels,
        BTreeSet::from(["About", "Enables", "Helps", "Until"])
    );
    // Edges left behind by a revision, and equal content on a stale and a
    // current root.
    assert!(view.edges().any(|edge| {
        view.is_stale(edge.id) == Some(true)
            && view
                .edges_labelled(&edge.ty_name)
                .any(|other| other.id != edge.id)
    }));
    assert!(view.nodes().any(|node| {
        node.predecessor.is_none()
            && view.is_stale(node.id) == Some(false)
            && view
                .nodes_with_content(&node.ty_name, None, &node.payload)
                .any(|other| view.is_stale(other.id) == Some(true))
    }));
}

fn every_analyzer() -> AnalysisRunner {
    let mut runner = AnalysisRunner::new(EvaluationInputs::default());
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    runner
        .register_analyzer_kind(Box::new(ReconnectFactory))
        .unwrap();
    runner
        .register_analyzer_kind(Box::new(DagCheckFactory))
        .unwrap();
    runner
}

// The default suite and DAG Check, unfiltered and filtered by type and
// label, which select their entries through different core indexes.
fn varied_suite() -> SuiteConfig {
    let mut suite = default_suite();
    for configuration in [
        r#"{"on_cycle":"warning"}"#,
        r#"{"on_cycle":"warning","filter":{"edge_types":["Enables","Helps"]}}"#,
        r#"{"on_cycle":"inconsistency","filter":{"node_types":["Task","Project","Habit"],"edge_types":["Enables"]}}"#,
    ] {
        suite.0.push(Config {
            name: DagCheckFactory::name().into(),
            configuration: configuration.into(),
        });
    }
    suite
}

// The variant of a TypeCheck certificate, which encodes the typed finding.
fn variant(certificate: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(certificate).unwrap();
    value.as_object().unwrap().keys().next().unwrap().clone()
}

// Every analyzer gives the same output with and without indexes over the
// varied histories, which raise findings of every analyzer.
#[test]
fn varied_histories_give_every_analyzer_the_same_output_under_every_indexing() {
    let mut runner = every_analyzer();
    let mut seen = BTreeMap::<String, usize>::new();
    for (entries, review_every, seed) in [
        (300, REVIEW_EVERY_TASK, 1),
        (300, REVIEW_EVERY_TASK, 2),
        (300, MIN_REVIEW_EVERY, 3),
        (300, 40, 4),
    ] {
        let graph = varied(entries, review_every, seed);
        let output =
            assert_indexing_equivalence(&mut runner, graph.view(), &varied_suite()).unwrap();
        for observation in output.observations() {
            let name = observation.key.analyzer.name.as_str();
            let kind = if name == TypeCheckFactory::name() {
                variant(&observation.key.analyzer_certificate.0)
            } else {
                name.to_owned()
            };
            *seen.entry(kind).or_default() += 1;
        }
    }
    let kinds = seen.keys().map(String::as_str).collect::<BTreeSet<_>>();
    for expected in [
        "BadEdgeEndpoint",
        "BadSuccessor",
        "DuplicateType",
        "Field",
        "MalformedDeclaration",
        "MissingType",
        "UnknownSuccessorType",
        ReconnectFactory::name(),
        DagCheckFactory::name(),
    ] {
        assert!(kinds.contains(expected), "no {expected} in {seen:?}");
    }
}

// The core queries with indexes against their reference scans at every
// boundary of a varied history, for a sample of identities of the head
// (some past each boundary) and every type and label it uses.
fn assert_indexed_queries_match(head: View<'_>) {
    let probes = head
        .entries()
        .map(Entry::id)
        .filter(|id| id.position() % 7 == 0)
        .collect::<Vec<_>>();
    let mut names = head
        .entries()
        .map(|entry| match entry {
        Entry::Node(node) => node.ty_name.clone(),
        Entry::Edge(edge) => edge.ty_name.clone(),
        })
        .collect::<BTreeSet<_>>();
    names.insert("Unused".into());
    let node_ids = |nodes: &mut dyn Iterator<Item = &lifecore::Node>| {
        nodes.map(|node| node.id).collect::<Vec<_>>()
    };
    let edge_ids = |edges: &mut dyn Iterator<Item = &lifecore::Edge>| {
        edges.map(|edge| edge.id).collect::<Vec<_>>()
    };
    for end in 0..=head.len() {
        let indexed = head.prefix(end).unwrap();
        let scanned = indexed.without_indexes();
        for &id in &probes {
            assert_eq!(indexed.is_stale(id), scanned.is_stale(id));
            assert_eq!(indexed.root_of(id), scanned.root_of(id));
            assert_eq!(
                node_ids(&mut indexed.children(id)),
                node_ids(&mut scanned.children(id))
            );
            assert_eq!(
                node_ids(&mut indexed.fresh_successors(id)),
                node_ids(&mut scanned.fresh_successors(id))
            );
            assert_eq!(
                node_ids(&mut indexed.descendants(id)),
                node_ids(&mut scanned.descendants(id))
            );
            assert_eq!(
                edge_ids(&mut indexed.edges_of(id)),
                edge_ids(&mut scanned.edges_of(id))
            );
        }
        for name in &names {
            assert_eq!(
                node_ids(&mut indexed.nodes_of_type(name)),
                node_ids(&mut scanned.nodes_of_type(name))
            );
            assert_eq!(
                edge_ids(&mut indexed.edges_labelled(name)),
                edge_ids(&mut scanned.edges_labelled(name))
            );
            assert_eq!(indexed.resolve_type(name), scanned.resolve_type(name));
        }
        assert_eq!(current_declarations(indexed), current_declarations(scanned));
    }
}

fn current_declarations(view: View<'_>) -> Vec<(Option<String>, lifecore::typing::Declaration)> {
    view.declarations()
        .iter()
        .map(|(name, declaration)| (name.map(str::to_owned), declaration.clone()))
        .collect()
}

#[test]
fn indexed_queries_match_their_scans_at_every_boundary_of_varied_histories() {
    for seed in [5, 6] {
        assert_indexed_queries_match(varied(170, REVIEW_EVERY_TASK, seed).view());
    }
}
