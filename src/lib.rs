use anyhow;
use arrayvec::{ArrayString, ArrayVec};
use jiff::{Timestamp, Unit, Zoned};
use serde::{Deserialize, Serialize};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    ops::Index,
};
use strum::{EnumIs, EnumTryAs};
use whoami::{hostname, username};


#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct EventIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Circumstance {
    // Timestamp with 1h resolution
    when_1h: Zoned,
    hostname: LongName,
    username: LongName,
}
impl Circumstance {
    pub fn current() -> anyhow::Result<Self> {
        let now = Zoned::now().round(Unit::Hour)?;
        let hname = hostname()?;
        let uname = username()?;
        // CANNOT actually use `?` because it would attempt to return a reference to strings allocated locally
        let hname_array = LongName::try_from(hname.as_str());
        let uname_array = LongName::try_from(uname.as_str());
        if hname_array.is_err() || uname_array.is_err() {
            anyhow::bail!("Hostname or Username too long!")
        }

        return Ok(Self {
            when_1h: now,
            hostname: hname_array.unwrap(),
            username: uname_array.unwrap(),
        });
    }
}

#[derive(Serialize, Deserialize)]
struct LifeCore {
    // indexed by EventIndex
    event: Vec<Event>,
    circumstance: Vec<CircumstanceIndex>,

    // indexed by CircumstanceIndex
    circumstance_table: Vec<Circumstance>,

    // indexed by StringIndex
    string_table: Vec<String>,
}

impl Index<EventIndex> for LifeCore {
    type Output = (Circumstance, Event);

    fn index(&self, index: EventIndex) -> &Self::Output {
        (self.circumstance_table[self.circumstance[index.0]],self.event[index.0])
    }

}

#[derive(Serialize, Deserialize, EnumIs, EnumTryAs, Clone, PartialEq, Eq, Hash)]
enum Event {
    Node(Node),
    Edge(Edge),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
struct Node {
    ty: Name,
    parent: EventIndex,
    payload: ArrayVec<Option<(Name, Payload)>, 32>,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
struct Edge {
    ty: Name,
    from: EventIndex,
    to: EventIndex,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),
}

mod lifecore;
#[cfg(test)]
mod tests;
