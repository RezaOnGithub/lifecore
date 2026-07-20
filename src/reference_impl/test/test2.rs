use std::sync::Once;

use env_logger;
use itertools::Itertools;
use log;

use crate::{
    BasicValidationFailure,
    BasicValidationFailureScenario::ExtraneousMockCircumstance,
    Body, Circumstance, Edge, EntryContext, LifeCore, LifeCoreError, LifeCoreList, Node,
    reference_impl::{XEdge, XEntry, XNode},
};

fn init_test_logging() {
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace"))
            .format_timestamp_millis()
            .is_test(false) // comment out when debugging crashes
            .target(env_logger::Target::Stderr)
            .try_init()
            .expect("Could not initialize logger");
    });
}

#[test]
fn context01() {
    let ctx = EventContext::universe();
    assert!(ctx.range().contains(&1024));
}

#[test]
fn raw_edge_window() {
    init_test_logging();
    let mut lc = LifeCoreList::default();
    let mut n: Vec<NodeExt> = vec![];
    let mut e: Vec<EdgeExt> = vec![];

    for x in 0..10 {
        n.push(
            lc.add(format!("N{}", x).as_str(), Body::from(&[]), None)
                .unwrap(),
        );
    }

    let w = || EventContext::universe().items();

    assert_eq!(lc.window(w()).count(), 10);
    assert_eq!(lc.edges(EventContext::universe()).count(), 0);
    assert_eq!(lc.raw_edges(EventContext::universe()).count(), 0);
    assert_eq!(lc.raw_edges(EventContext::universe()).unique().count(), 0);

    e.push(lc.connect(n[0].e, "Some_Edge", n[1].e).unwrap());
    assert!(lc.basic_validation().is_ok());
    assert_eq!(w().zip(lc.window(w())).count(), 11);
    assert_eq!(lc.raw_edges(EventContext::universe()).count(), 1);
    assert_eq!(lc.raw_edges(EventContext::universe()).unique().count(), 1);
    assert_eq!(lc.edges(EventContext::universe()).count(), 1);
}

#[test]
fn mock_events_form_a_validated_tail_suffix() {
    let mut lc = LifeCoreList::default();
    let _committed = lc.add("Committed", Body::default(), None).unwrap();
    let first = lc.add_mock("First_Mock", Body::default(), None).unwrap();
    let second = lc.add_mock("Second_Mock", Body::default(), None).unwrap();
    let edge = lc.connect_mock(first.e, "Mock_Edge", second.e).unwrap();

    assert!(lc.is_mock());
    assert!(lc.basic_validation().is_ok());
    for event in [first.e, second.e, edge.e] {
        assert!(matches!(
            lc.get_with_circumstance(event).unwrap().1,
            Circumstance::Mock
        ));
    }

    let before_rejected_add = lc.clone();
    let error = lc
        .add("Committed_After_Mock", Body::default(), None)
        .unwrap_err();
    assert!(matches!(
        error,
        LifeCoreError::BasicValidationFailure(BasicValidationFailure {
            reason: ExtraneousMockCircumstance,
            ..
        })
    ));
    assert!(lc == before_rejected_add);
}

#[test]
fn apply_commits_the_mock_suffix_without_shifting_prior_circumstances() {
    let mut lc = LifeCoreList::default();
    let committed = lc.add("Committed", Body::default(), None).unwrap();
    let prior_circumstance_index = lc.circumstance[committed.e.thing];
    let prior_circumstance = lc[prior_circumstance_index].clone();
    assert!(matches!(&prior_circumstance, Circumstance::Session { .. }));
    let first = lc.add_mock("First_Mock", Body::default(), None).unwrap();
    let second = lc
        .add_mock("Second_Mock", Body::default(), Some(first.e))
        .unwrap();
    let staged_events = [lc.get(first.e).unwrap(), lc.get(second.e).unwrap()];

    lc.apply().unwrap();

    assert!(!lc.is_mock());
    assert!(lc.basic_validation().is_ok());
    assert!(lc.circumstance[committed.e.thing] == prior_circumstance_index);
    assert!(lc[prior_circumstance_index] == prior_circumstance);
    let first_circumstance = lc.get_with_circumstance(first.e).unwrap().1;
    let second_circumstance = lc.get_with_circumstance(second.e).unwrap().1;
    assert!(matches!(&first_circumstance, Circumstance::Session { .. }));
    assert!(first_circumstance == second_circumstance);
    assert_eq!(lc.get(first.e).unwrap(), staged_events[0]);
    assert_eq!(lc.get(second.e).unwrap(), staged_events[1]);

    let applied = lc.clone();
    lc.apply().unwrap();
    assert!(lc == applied);
}
