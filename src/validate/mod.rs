use crate::*;
use itertools::Itertools;

impl LifeCore {
    // Rule: exact edge duplicate events are forbidden
    // Rule: exact node duplicate events are fobidden if both are valid within the neighborhood
    // node duplicates are discouraged otherwise
    pub fn validate_no_duplicates(&self) -> bool {
        // Check edges
        {
            let t = self
                .event
                .iter()
                .map(|x| x.1.clone())
                .filter(|x| x.is_edge());
            let edge_count = t.clone().count();
            let unique_edge_count = t.clone().unique().count();
            if edge_count != unique_edge_count {
                return false;
            }
        }

        // Hard part: check nodes
        let dupe_class = self
            .event
            .iter()
            .enumerate()
            .filter(|x| x.1.1.is_node())
            .map(|x| (x.0, (x.1.0, x.1.1.clone().try_as_node().unwrap())))
            .into_group_map_by(|x| x.1.1.clone())
            .into_values()
            .filter(|bucket| bucket.len() > 1)
            .collect_vec();

        for dupe in dupe_class {
            for pair in dupe.iter().combinations(2) {
                let (older, newer) = {
                    if pair[0].0 < pair[1].0 {
                        (pair[0], pair[1])
                    } else {
                        (pair[1], pair[0])
                    }
                };

                let mut older_invalidated = false;
                for interim in self.event[(older.0 + 1)..newer.0]
                    .iter()
                    .filter(|x| x.1.is_node())
                    .map(|x| (x.0, x.1.clone().try_as_node().unwrap()))
                {
                    if interim.1.parent.0 == older.0 {
                        older_invalidated = true;
                        break;
                    }
                }

                if older_invalidated {
                    eprintln!("Duplicate nodes, but older one was invalidated later (still discouraged). EventIndex {} and {}", older.0, newer.0);
                } else {
                    return false;
                }
            }
        }

        true
    }

    // Rule: events are sorted, nodes and edges cannot refer to themselves or future
    // Rule: no dangling IDs
    pub fn validate_basic_id_ordering(&self) -> bool {
        for e in self.event.iter() {
            if let Event::Edge(Edge { ty: _, from, to }) = e.1 {
                if from >= e.0
                    || to >= e.0
                    || !self.event.contains_key(from)
                    || !self.event.contains_key(to)
                {
                    return false;
                }
            } else {
                let parent_id = e.1.clone().try_as_node().unwrap().parent;
                if parent_id >= *e.0 || !self.event.contains_key(&parent_id) {
                    return false;
                }
            }
        }
        true
    }

    // Rule: after a node has been overriden, (an event is pushed where it is the parent of another node)
    //       no edges can go from or to that node
    pub fn validate_edge_stale_rule(&self) -> bool {
        for edge_id in self
            .event
            .keys()
            .filter(|x| self.event[x].is_edge())
            .sorted()
        {
            let edge = self.event[edge_id].clone().try_as_edge().unwrap();
            let is_overriden = |node_id: Id| -> bool {
                for interim_id in self
                    .event
                    .keys()
                    .filter(|x| self.event[x].is_node() && **x >= node_id && *x <= edge_id)
                    .sorted()
                {
                    let interim_node_parent =
                        self.event[interim_id].clone().try_as_node().unwrap().parent;
                    if interim_node_parent == node_id {
                        return true;
                    }
                }
                false
            };
            if is_overriden(edge.from) || is_overriden(edge.to) {
                return false;
            }
        }
        true
    }
}
