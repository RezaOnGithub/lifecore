// Core indexes against their reference scans (SPEC §9.1), and
// incremental validation against validating the whole history, with and
// without indexes. `lifecore` cannot depend on the workload crate, so the
// histories come from a small generator of its own.

use crate::{
    BasicValidationFailure, Body, Edge, Entry, EntryId, LifeCoreError, LifeCoreList, Node, Payload,
    View,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerCertificate, AnalyzerFactory, Config, Diagnostic,
        EvaluationInputs, Indexing, Message, MessageKind, SuiteConfig, assert_indexing_equivalence,
    },
    inspection::{Currentness, Direction, EdgeSelection, NodeSelection},
    typing::{Declaration, FieldStatus, Type, ValueType},
};
use semver::Version;
use std::collections::BTreeMap;
use uuid::Uuid;

// splitmix64: small, deterministic, and good enough to vary a history.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

// Type names and labels the generator uses, and some it never does.
const NODE_TYPES: [&str; 3] = ["A", "B", Type::TYPE_NAME];
const LABELS: [&str; 2] = ["x", "y"];
const PROBE_NAMES: [&str; 7] = ["A", "B", Type::TYPE_NAME, "x", "y", "Unused", ""];

// Declarations that parse, claim a reserved name, or are malformed with and
// without a claim, so K8 caches both outcomes and names collide.
fn declaration(rng: &mut Rng) -> Body {
    use FieldStatus::Optional;
    match rng.below(7) {
    0 => Type::new("A", &[], &["x"], &["x"], &["B"]).to_body(),
    1 => Type::new(
        "A",
        &[("n", (ValueType::Integer, Optional))],
        &["x", "y"],
        &["x"],
        &[],
    )
    .to_body(),
    2 => Type::new("B", &[], &[], &["x", "y"], &[]).to_body(),
    3 => Type::new(Type::TYPE_NAME, &[], &[], &[], &[]).to_body(),
    4 => Body::from([
        ("name".into(), "A".into()),
        ("bogus".into(), Payload::Integer(1)),
    ]),
    5 => Body::new(),
    _ => Body::from([("name".into(), Payload::Integer(3))]),
    }
}

// A small payload domain, so equal contents recur, both while an earlier one
// is current (a duplicate) and after it was superseded (permitted).
fn payload(rng: &mut Rng, ty_name: &str) -> Body {
    if ty_name == Type::TYPE_NAME {
        return declaration(rng);
    }
    match rng.below(4) {
    0 => Body::new(),
    n => Body::from([("n".into(), Payload::Integer(n as i64))]),
    }
}

struct Generator {
    rng: Rng,
    // Identities of another database: same positions, other origins.
    foreign: Vec<EntryId>,
    origin: Uuid,
}

impl Generator {
    fn new(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let foreign_origin = Uuid::from_u128(u128::from(rng.next()) | 1);
        Self {
            foreign: (0..4)
                .map(|position| EntryId {
                    position,
                    origin: foreign_origin,
                })
                .collect(),
            origin: Uuid::from_u128(u128::from(rng.next()) << 1 | 1),
            rng,
        }
    }

    // Mostly an entry of the view, preferring current nodes; sometimes a
    // foreign identity, an edge, or a position past the view.
    fn reference(&mut self, view: View<'_>) -> EntryId {
        let fresh = view.fresh_nodes().map(|node| node.id).collect::<Vec<_>>();
        match self.rng.below(24) {
        0 => self.rng.pick(&self.foreign),
        1 => EntryId {
            position: view.len() + self.rng.below(2),
            origin: self.origin,
        },
        _ if !fresh.is_empty() && !self.rng.one_in(3) => self.rng.pick(&fresh),
        _ if !view.is_empty() => view.at(self.rng.below(view.len())).unwrap().id(),
        _ => self.rng.pick(&self.foreign),
        }
    }

    // The next attempted append, valid or not.
    fn attempt(&mut self, view: View<'_>) -> Entry {
        if self.rng.one_in(6) {
            self.origin = Uuid::from_u128(u128::from(self.rng.next()) << 1 | 1);
        }
        let id = EntryId {
            position: view.len() + usize::from(self.rng.one_in(50)),
            origin: if self.rng.one_in(50) {
                Uuid::nil()
            } else {
                self.origin
            },
        };
        if self.rng.below(9) < 5 {
            let ty_name = self.rng.pick(&NODE_TYPES).to_owned();
            let predecessor = (!self.rng.one_in(3)).then(|| self.reference(view));
            Entry::Node(Node {
                id,
                payload: payload(&mut self.rng, &ty_name),
                ty_name,
                predecessor,
            })
        } else {
            let from = self.reference(view);
            let to = if self.rng.one_in(20) {
                from
            } else {
                self.reference(view)
            };
            Entry::Edge(Edge {
                id,
                ty_name: self.rng.pick(&LABELS).into(),
                from,
                to,
            })
        }
    }
}

// The outcome of one attempt, for counting variety.
fn outcome(result: &Result<(), BasicValidationFailure>, entry: &Entry) -> &'static str {
    use BasicValidationFailure::*;
    match (result, entry) {
    (Ok(()), Entry::Node(node)) if node.predecessor.is_some() => "successor",
    (Ok(()), Entry::Node(_)) => "root",
    (Ok(()), Entry::Edge(_)) => "edge",
    (Err(InvalidIdentity { .. }), _) => "invalid identity",
    (Err(InvalidReference { .. }), _) => "invalid reference",
    (Err(Duplicate { .. }), Entry::Node(_)) => "duplicate node",
    (Err(Duplicate { .. }), Entry::Edge(_)) => "duplicate edge",
    (Err(StaleEndpoint { .. }), _) => "stale endpoint",
    (Err(SelfEdge { .. }), _) => "self edge",
    (Err(InvalidDatabase), _) => "invalid database",
    }
}

// Stages `entry` and asserts the incremental result equals validating the
// whole extended history, with and without indexes. Debug builds also compare
// the extended indexes with rebuilt ones inside `stage`.
fn stage_like_full_validation(
    graph: &mut LifeCoreList,
    entry: Entry,
) -> Result<(), BasicValidationFailure> {
    let view = graph.view();
    let mut entries = view.entries().cloned().collect::<Vec<_>>();
    entries.push(entry.clone());
    let full = LifeCoreList::validate_stored(view.id().database, entries);
    let before = graph.clone();
    let staged = graph.stage(entry);
    assert_eq!(full, [staged.clone(), staged.clone()]);
    if staged.is_err() {
        assert_eq!(*graph, before);
        assert_eq!(graph.staleness(), before.staleness());
    }
    staged
}

fn generate(
    seed: u64,
    attempts: usize,
) -> (LifeCoreList, Vec<EntryId>, BTreeMap<&'static str, usize>) {
    let mut generator = Generator::new(seed);
    let mut graph = LifeCoreList::default();
    let mut outcomes = BTreeMap::new();
    for _ in 0..attempts {
        let entry = generator.attempt(graph.view());
        let kind = {
            let result = stage_like_full_validation(&mut graph, entry.clone());
            outcome(&result, &entry)
        };
        *outcomes.entry(kind).or_default() += 1;
    }
    let mut probes = generator.foreign.clone();
    probes.push(EntryId {
        position: graph.view().len(),
        origin: generator.origin,
    });
    (graph, probes, outcomes)
}

fn ids<'a, T: 'a>(items: impl Iterator<Item = &'a T>, id: impl Fn(&T) -> EntryId) -> Vec<EntryId> {
    items.map(id).collect()
}
fn node_ids<'a>(nodes: impl Iterator<Item = &'a Node>) -> Vec<EntryId> {
    ids(nodes, |node| node.id)
}
fn edge_ids<'a>(edges: impl Iterator<Item = &'a Edge>) -> Vec<EntryId> {
    ids(edges, |edge| edge.id)
}

fn declarations(view: View<'_>) -> Vec<(Option<&str>, Declaration)> {
    view.declarations()
        .iter()
        .map(|(name, declaration)| (name, declaration.clone()))
        .collect()
}

// Every query of every prefix, indexed against unindexed, for every identity
// of the head view (so also those past each boundary), every probe, and
// every name the generator uses or never uses.
fn assert_queries_agree(head: View<'_>, probes: &[EntryId]) {
    let probes = head
        .entries()
        .map(Entry::id)
        .chain(probes.iter().copied())
        .collect::<Vec<_>>();
    for end_exclusive in 0..=head.len() {
        let indexed = head.prefix(end_exclusive).unwrap();
        let scanned = indexed.without_indexes();
        let indexes = indexed.indexes().expect("a graph's view carries indexes");
        assert!(scanned.indexes().is_none());
        assert_eq!(indexes.end_exclusive(), end_exclusive);
        let at = |what: &str, id: Option<EntryId>| format!("{what} {id:?} at {end_exclusive}");
        for &id in &probes {
            assert_eq!(
                indexed.is_stale(id),
                scanned.is_stale(id),
                "{}",
                at("is_stale", Some(id))
            );
            assert_eq!(
                indexes.has_child(id),
                scanned.node(id).map(|_| scanned.is_stale(id) == Some(true)),
                "{}",
                at("has_child", Some(id))
            );
            assert_eq!(
                node_ids(indexed.children(id)),
                node_ids(scanned.children(id)),
                "{}",
                at("children", Some(id))
            );
            assert_eq!(
                edge_ids(indexed.edges_of(id)),
                edge_ids(scanned.edges_of(id)),
                "{}",
                at("edges_of", Some(id))
            );
            assert_eq!(
                node_ids(indexed.fresh_successors(id)),
                node_ids(scanned.fresh_successors(id)),
                "{}",
                at("fresh_successors", Some(id))
            );
            assert_eq!(
                node_ids(indexed.descendants(id)),
                node_ids(scanned.descendants(id)),
                "{}",
                at("descendants", Some(id))
            );
            assert_eq!(
                indexed.root_of(id),
                scanned.root_of(id),
                "{}",
                at("root_of", Some(id))
            );
            assert_eq!(
                indexes.declaration(id),
                scanned
                    .node(id)
                    .filter(|node| node.ty_name == Type::TYPE_NAME)
                    .map(Declaration::of)
                    .as_ref(),
                "{}",
                at("declaration", Some(id))
            );
            for direction in [Direction::Incoming, Direction::Outgoing, Direction::Both] {
                for ty_name in [None, Some("x")] {
                    let selection = EdgeSelection {
                        ty_name,
                        currentness: Currentness::All,
                    };
                    assert_eq!(
                        edge_ids(indexed.relationships(id, direction, selection)),
                        edge_ids(scanned.relationships(id, direction, selection)),
                    );
                }
            }
        }
        for name in PROBE_NAMES {
            assert_eq!(
                node_ids(indexed.nodes_of_type(name)),
                node_ids(scanned.nodes_of_type(name)),
                "{}",
                at(name, None)
            );
            assert_eq!(
                edge_ids(indexed.edges_labelled(name)),
                edge_ids(scanned.edges_labelled(name)),
                "{}",
                at(name, None)
            );
            for currentness in [
                Currentness::Current,
                Currentness::Historical,
                Currentness::All,
            ] {
                let nodes = NodeSelection {
                    ty_name: Some(name),
                    currentness,
                    ..NodeSelection::default()
                };
                assert_eq!(
                    node_ids(indexed.select_nodes(&nodes)),
                    node_ids(scanned.select_nodes(&nodes))
                );
                let edges = EdgeSelection {
                    ty_name: Some(name),
                    currentness,
                };
                assert_eq!(
                    edge_ids(indexed.select_edges(edges)),
                    edge_ids(scanned.select_edges(edges))
                );
            }
            assert_eq!(indexed.resolve_type(name), scanned.resolve_type(name));
        }
        // Contents and triples of every entry of the head, so also keys that
        // occur only past the boundary, and a reversed triple.
        for entry in head.entries() {
            match entry {
            Entry::Node(node) => assert_eq!(
                node_ids(indexed.nodes_with_content(
                    &node.ty_name,
                    node.predecessor,
                    &node.payload
                )),
                node_ids(scanned.nodes_with_content(
                    &node.ty_name,
                    node.predecessor,
                    &node.payload
                )),
                "{}",
                at("content of", Some(node.id))
            ),
            Entry::Edge(edge) => {
                for (from, to) in [(edge.from, edge.to), (edge.to, edge.from)] {
                    assert_eq!(
                        indexed.edge_between(from, to, &edge.ty_name),
                        scanned.edge_between(from, to, &edge.ty_name),
                        "{}",
                        at("triple of", Some(edge.id))
                    );
                }
            }
            }
        }
        assert_eq!(
            node_ids(indexed.fresh_nodes()),
            node_ids(scanned.fresh_nodes())
        );
        assert_eq!(
            edge_ids(indexed.fresh_edges()),
            edge_ids(scanned.fresh_edges())
        );
        assert_eq!(declarations(indexed), declarations(scanned));
        assert_eq!(
            indexed.declarations().names().collect::<Vec<_>>(),
            scanned.declarations().names().collect::<Vec<_>>()
        );
        assert_eq!(indexed.inspection(), scanned.inspection());
    }
}

#[test]
fn generated_histories_stage_like_full_validation_and_every_index_matches_its_scan() {
    let mut seen = BTreeMap::<&str, usize>::new();
    for seed in 0..8 {
        let (graph, probes, outcomes) = generate(seed, 110);
        for (kind, count) in outcomes {
            *seen.entry(kind).or_default() += count;
        }
        graph.basic_validation().unwrap();
        assert_queries_agree(graph.view(), &probes);
        // A reloaded history has rebuilt indexes and answers alike.
        let loaded: LifeCoreList =
            serde_json::from_value(serde_json::to_value(&graph).unwrap()).unwrap();
        assert_eq!(loaded, graph);
        assert_eq!(loaded.staleness(), graph.staleness());
    }
    // The generator reaches every outcome the indexes take part in, except a
    // nil database, which only a test can construct.
    let kinds = seen.keys().copied().collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "duplicate edge",
            "duplicate node",
            "edge",
            "invalid identity",
            "invalid reference",
            "root",
            "self edge",
            "stale endpoint",
            "successor",
        ],
        "{seen:?}"
    );
}

fn id(position: usize, origin: Uuid) -> EntryId {
    EntryId { position, origin }
}
fn node(position: usize, origin: Uuid, ty_name: &str, predecessor: Option<EntryId>) -> Entry {
    Entry::Node(Node {
        id: id(position, origin),
        ty_name: ty_name.into(),
        predecessor,
        payload: Body::new(),
    })
}
fn edge(position: usize, origin: Uuid, from: EntryId, to: EntryId) -> Entry {
    Entry::Edge(Edge {
        id: id(position, origin),
        ty_name: "x".into(),
        from,
        to,
    })
}

// Each failure at a known position, validated three ways: the whole history
// with indexes, the whole history by scans, and staging the failing entry on
// the valid history before it.
#[test]
fn validation_gives_the_same_first_failure_with_and_without_indexes() {
    assert_first_failures_agree();
}

// The same with every content hash colliding, so the duplicate rules and K5
// and K6 decide by complete values alone.
#[test]
fn validation_and_every_index_agree_when_all_content_hashes_collide() {
    let _colliding = crate::index::Colliding::start();
    assert_first_failures_agree();
    let mut seen = BTreeMap::<&str, usize>::new();
    for seed in [11, 12, 13] {
        let (graph, probes, outcomes) = generate(seed, 90);
        for (kind, count) in outcomes {
            *seen.entry(kind).or_default() += count;
        }
        graph.basic_validation().unwrap();
        assert_queries_agree(graph.view(), &probes);
    }
    assert!(seen.contains_key("duplicate node"), "{seen:?}");
    assert!(seen.contains_key("duplicate edge"), "{seen:?}");
}

fn assert_first_failures_agree() {
    use BasicValidationFailure::*;
    let o = Uuid::from_u128(7);
    let a = id(0, o);
    let b = id(1, o);
    let a2 = id(2, o);
    let base = || {
        vec![
            node(0, o, "A", None),
            node(1, o, "B", None),
            node(2, o, "A", Some(a)),
            edge(3, o, a2, b),
        ]
    };
    let cases: Vec<(Entry, BasicValidationFailure)> = vec![
        (node(5, o, "C", None), InvalidIdentity { position: 4 }),
        (
            node(4, Uuid::nil(), "C", None),
            InvalidIdentity { position: 4 },
        ),
        (
            node(4, o, "C", Some(id(1, Uuid::from_u128(8)))),
            InvalidReference {
                entry: id(4, o),
                target: id(1, Uuid::from_u128(8)),
            },
        ),
        (
            node(4, o, "C", Some(id(3, o))),
            InvalidReference {
                entry: id(4, o),
                target: id(3, o),
            },
        ),
        (
            node(4, o, "C", Some(id(4, o))),
            InvalidReference {
                entry: id(4, o),
                target: id(4, o),
            },
        ),
        (node(4, o, "B", None), Duplicate { entry: id(4, o) }),
        (node(4, o, "A", Some(a)), Duplicate { entry: id(4, o) }),
        (edge(4, o, a, b), StaleEndpoint { entry: id(4, o) }),
        (edge(4, o, b, a), StaleEndpoint { entry: id(4, o) }),
        (edge(4, o, b, b), SelfEdge { entry: id(4, o) }),
        (edge(4, o, a2, b), Duplicate { entry: id(4, o) }),
    ];
    // A stale duplicate root and the reversed edge are permitted.
    for valid in [node(4, o, "A", None), edge(4, o, b, a2)] {
        let mut entries = base();
        entries.push(valid.clone());
        assert_eq!(
            LifeCoreList::validate_stored(o, entries.clone()),
            [Ok(()), Ok(())]
        );
        let mut graph = LifeCoreList::empty(o);
        for entry in entries {
            stage_like_full_validation(&mut graph, entry).unwrap();
        }
    }
    for (failing, failure) in cases {
        let mut graph = LifeCoreList::empty(o);
        for entry in base() {
            stage_like_full_validation(&mut graph, entry).unwrap();
        }
        assert_eq!(
            stage_like_full_validation(&mut graph, failing.clone()),
            Err(failure.clone())
        );
        // Entries after the first failure change nothing.
        let mut entries = base();
        entries.extend([failing, node(5, o, "B", None), edge(6, o, b, b)]);
        assert_eq!(
            LifeCoreList::validate_stored(o, entries),
            [Err(failure.clone()), Err(failure)]
        );
    }
    // A nil database fails before any entry, by both paths and on staging.
    assert_eq!(
        LifeCoreList::validate_stored(Uuid::nil(), vec![]),
        [Err(InvalidDatabase), Err(InvalidDatabase)]
    );
    let mut nil = LifeCoreList::empty(Uuid::nil());
    assert_eq!(
        stage_like_full_validation(&mut nil, node(0, o, "A", None)),
        Err(InvalidDatabase)
    );
}

// Indexes take no part in any channel through which a view or a graph is
// observed, and their own `Debug` shows the boundary only.
#[test]
fn indexes_are_invisible_to_debug_equality_and_serialization() {
    let (graph, _, _) = generate(3, 60);
    let view = graph.view();
    for end_exclusive in [0, view.len() / 2, view.len()] {
        let prefix = view.prefix(end_exclusive).unwrap();
        assert_eq!(
            format!("{prefix:?}"),
            format!("{:?}", prefix.without_indexes())
        );
        assert_eq!(
            format!("{:?}", prefix.indexes().unwrap()),
            format!("Indexes {{ end_exclusive: {end_exclusive}, .. }}")
        );
        assert_eq!(prefix.id(), prefix.without_indexes().id());
    }
    let stored = serde_json::to_value(&graph).unwrap();
    assert_eq!(
        stored.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["entries", "id", "suite"]
    );
    // The same history staged with colliding hashes holds other K5 and K6
    // contents, and is still equal, and prints alike.
    let colliding = {
        let _colliding = crate::index::Colliding::start();
        let mut staged = LifeCoreList::empty(view.id().database);
        for entry in view.entries() {
            staged.stage(entry.clone()).unwrap();
        }
        staged
    };
    assert_eq!(colliding, graph);
    assert_eq!(format!("{colliding:?}"), format!("{graph:?}"));
}

// A probe that reports whether its view carries indexes: the one thing an
// analyzer must never let change its output, here to show the modes.
struct IndexProbe;
impl Analyzer for IndexProbe {
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(vec![])
    }
    fn analyze(&self, view: View<'_>, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(view
            .indexes()
            .map(|indexes| {
                assert_eq!(indexes.end_exclusive(), view.len());
                Message {
                    analyzer_certificate: AnalyzerCertificate("indexed".into()),
                    diagnostic: Diagnostic {
                        subjects: Default::default(),
                        focus: None,
                        message: "indexed".into(),
                        kind: MessageKind::Warning,
                        sticky: false,
                    },
                }
            })
            .into_iter()
            .collect())
    }
}
struct IndexProbeFactory;
impl AnalyzerFactory for IndexProbeFactory {
    fn analyzer_with_config(&mut self, _: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(IndexProbe))
    }
    fn analyzer_name(&self) -> &'static str {
        "IndexProbe"
    }
}

#[test]
fn indexing_modes_choose_what_analyzers_see_and_the_helper_catches_a_difference() {
    let (graph, _, _) = generate(5, 30);
    let suite = SuiteConfig(vec![Config {
        name: "IndexProbe".into(),
        configuration: String::new(),
    }]);
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(IndexProbeFactory))
        .unwrap();
    assert_eq!(runner.indexing(), Indexing::All);
    let indexed = |runner: &mut AnalysisRunner, view: View<'_>| {
        runner
            .run_view(view, &suite)
            .unwrap()
            .observations()
            .count()
            == 1
    };
    for (indexing, expected) in [
        (Indexing::All, true),
        (Indexing::Core, true),
        (Indexing::Off, false),
    ] {
        runner.set_indexing(indexing);
        assert_eq!(indexed(&mut runner, graph.view()), expected, "{indexing:?}");
        // The runner builds core indexes for a view without them.
        assert_eq!(
            indexed(&mut runner, graph.view().without_indexes()),
            expected,
            "{indexing:?}"
        );
    }
    runner.set_indexing(Indexing::Core);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_indexing_equivalence(&mut runner, graph.view(), &suite)
    }));
    assert!(caught.is_err());
    // An analyzer that reads no index passes, failures included, and the
    // runner keeps its mode.
    let mut empty = AnalysisRunner::default();
    empty.set_indexing(Indexing::Off);
    assert!(assert_indexing_equivalence(&mut empty, graph.view(), &SuiteConfig::default()).is_ok());
    assert_eq!(
        assert_indexing_equivalence(&mut empty, graph.view(), &suite),
        Err(LifeCoreError::AnalyzerNotFound {
            name: "IndexProbe".into()
        })
    );
    assert_eq!(empty.indexing(), Indexing::Off);
}
