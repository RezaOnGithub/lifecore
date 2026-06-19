use arrayvec::{ArrayString, ArrayVec};
use jiff::{Timestamp, Unit, Zoned};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
};
use strum::{EnumIs, EnumTryAs};
use whoami::{hostname, username};

fn hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

type Name = ArrayString<64>;

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceHash(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct EventIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Circumstance {
    // Timestamp with 1h resolution
    when_1h: Zoned,
    hostname: String,
    username: String,
}
impl Circumstance {
    pub fn current() -> Option<Self> {
        let now = Zoned::now().round(Unit::Hour);
        let hname = hostname();
        let uname = username();
        if now.is_err() || hname.is_err() || uname.is_err() {
            return None;
        } else {
            return Some(Self {
                when_1h: now.unwrap(),
                hostname: hname.unwrap(),
                username: uname.unwrap(),
            });
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LifeCore {
    event: Vec<(CircumstanceHash, Event)>,
    circumstance: HashSet<Circumstance>,
}

#[derive(Serialize, Deserialize, EnumIs, EnumTryAs, Clone, PartialEq, Eq, Hash)]
enum Event {
    Node(Node),
    Edge(Edge),
}

#[derive(Serialize, Deserialize, Clone,  PartialEq, Eq, Hash)]
struct Node {
    ty: Name,
    parent: EventIndex,
    payload: ArrayVec<Option<(Name, Payload)>, 32>,
}
#[derive(Serialize, Deserialize, Clone,  PartialEq, Eq, Hash)]
struct Edge {
    ty: Name,
    from: EventIndex,
    to: EventIndex,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
enum Payload {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
}


#[cfg(test)]
mod tests;
mod validate;
