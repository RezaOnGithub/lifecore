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
            .is_test(false)
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
    let mut lc = LifeCoreList::new();
    let mut n: Vec<NodeExt> = vec![];
    let mut e: Vec<EdgeExt> = vec![];

    log::debug!("Alive 0");
    {
        let ty = lc.get_or_insert_string("Some_Node");
        lc.event.push(Event::Node(Node {
            ty,
            parent: None,
            payload: NodeBody::new(),
        }));
        log::debug!("Aha");
        for x in lc.window(EventContext::universe().index_iter()) {
            log::debug!("Hrm {:#?}", x);
        }
    }
    for x in 0..2 {
        log::debug!("Loop");
        n.push(
            lc.add(format!("N{}", x).as_str(), NodeBodyExt::from(&[]), None)
                .unwrap(),
        );
    }

    let w = || EventContext::universe().index_iter();

    // assert_eq!(lc.window(w()).count(), 10);
    log::warn!("Alive 1");

    // assert_eq!(lc.edges(EventContext::universe()).count(), 0);
    // log::warn!("Alive 2");

    // assert_eq!(lc.raw_edges(EventContext::universe()).count(), 0);
    // log::warn!("Alive 3");

    // assert_eq!(lc.raw_edges(EventContext::universe()).unique().count(), 0);
    // log::warn!("Alive 4");

    let ty = lc.get_or_insert_string("Some_Edge");
    lc.event.push(Event::Edge(Edge {
        ty,
        from: n[0].e.into(),
        to: n[1].e.into(),
    }));
    assert!(lc.basic_validation().is_success());

    // assert_eq!(w().zip(lc.window(w())).count(), 11);
    // assert_eq!(lc.raw_edges(EventContext::universe()).count(), 1);
    // assert_eq!(lc.raw_edges(EventContext::universe()).unique().count(), 1);
    // assert_eq!(lc.edges(EventContext::universe()).count(), 1);
}
