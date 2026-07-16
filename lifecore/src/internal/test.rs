use std::sync::Once;

use env_logger;
use itertools::Itertools;
use log;

use crate::{
    EdgeExt, EventContext, LifeCore, LifeCoreList, NodeBodyExt, NodeExt,
    internal::{Edge, Event, Node, NodeBody},
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
            lc.add(format!("N{}", x).as_str(), NodeBodyExt::from(&[]), None)
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
