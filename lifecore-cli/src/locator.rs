use std::{fmt, str::FromStr};

use lifecore::{Circumstance, Entry, EntryIndex, LifeCore, LifeCoreAccess, LifeCoreList};
use serde::Serialize;
use sha3::{Digest, Sha3_512};

use crate::{
    error::AppError,
    messages::encode_lower_hex,
    state::{CliStateV1, ENTRY_FINGERPRINT_VERSION, StoredEntryLocatorV1},
};

pub const DISPLAY_FINGERPRINT_LENGTH: usize = 16;
pub const MINIMUM_FINGERPRINT_LENGTH: usize = 5;
pub const FULL_FINGERPRINT_LENGTH: usize = 128;

#[derive(Serialize)]
enum FingerprintKind<'a> {
    Node {
        type_name: &'a str,
        top_level_field_count: u64,
    },
    Edge {
        type_name: &'a str,
    },
}

#[derive(Serialize)]
struct FingerprintInput<'a> {
    domain: &'a str,
    version: u32,
    state_uuid: &'a [u8; 16],
    index: u64,
    kind: FingerprintKind<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryFingerprint(String);

impl EntryFingerprint {
    pub fn for_entry(
        state: &CliStateV1,
        index: EntryIndex,
        entry: &Entry,
    ) -> Result<Self, AppError> {
        let bytes = entry_fingerprint_identity_bytes(state, index, entry)?;

        let digest: [u8; 64] = Sha3_512::digest(bytes).into();

        Ok(Self(encode_lower_hex(&digest)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn display_prefix(&self) -> &str {
        &self.0[..DISPLAY_FINGERPRINT_LENGTH]
    }
}

pub(crate) fn entry_fingerprint_identity_bytes(
    state: &CliStateV1,
    index: EntryIndex,
    entry: &Entry,
) -> Result<Vec<u8>, AppError> {
    let index = u64::try_from(index.0)
        .map_err(|_| AppError::invalid_state("entry index cannot be encoded as u64"))?;
    let kind = match entry {
    Entry::Node(node) => FingerprintKind::Node {
        type_name: &node.ty_name,
        top_level_field_count: u64::try_from(node.field.len())
            .map_err(|_| AppError::invalid_state("node field count cannot be encoded as u64"))?,
    },
    Entry::Edge(edge) => FingerprintKind::Edge {
        type_name: &edge.ty_name,
    },
    };
    let input = FingerprintInput {
        domain: "lifecore-cli-entry-fingerprint",
        version: state.entry_fingerprint_version,
        state_uuid: state.uuid.as_bytes(),
        index,
        kind,
    };

    //A Postcard gives the checksum fields an explicit, unambiguous wire shape.
    //A Changing this DTO is a fingerprint-version change.
    postcard::to_allocvec(&input)
        .map_err(|error| AppError::invalid_state(format!("fingerprint encoding failed: {error}")))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryLocator {
    index: usize,
    prefix: String,
}

impl EntryLocator {
    pub fn parse(text: &str) -> Result<Self, AppError> {
        text.parse()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

impl FromStr for EntryLocator {
    type Err = AppError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let Some((index, prefix)) = text.split_once(':') else {
            return Err(AppError::invalid_entry_locator(
                "expected INDEX:FINGERPRINT",
            ));
        };

        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AppError::invalid_entry_locator(
                "index must contain only ASCII decimal digits",
            ));
        }
        let index = index
            .parse::<usize>()
            .map_err(|_| AppError::invalid_entry_locator("entry index overflows this platform"))?;

        if !(MINIMUM_FINGERPRINT_LENGTH..=FULL_FINGERPRINT_LENGTH).contains(&prefix.len())
            || !prefix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AppError::invalid_entry_locator(format!(
                "fingerprint must contain {MINIMUM_FINGERPRINT_LENGTH} to \
                 {FULL_FINGERPRINT_LENGTH} lowercase hexadecimal digits"
            )));
        }

        Ok(Self {
            index,
            prefix: prefix.to_string(),
        })
    }
}

impl fmt::Display for EntryLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.index, self.prefix)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryExpectation {
    Any,
    Node,
}

#[derive(Clone)]
pub struct ResolvedEntry {
    pub entry: Entry,
    pub circumstance: Circumstance,
    pub fingerprint: EntryFingerprint,
}

impl ResolvedEntry {
    pub fn index(&self) -> usize {
        self.entry.entry_context().index().0
    }

    pub fn full_locator(&self) -> String {
        format!("{}:{}", self.index(), self.fingerprint.as_str())
    }

    pub fn display_locator(&self) -> String {
        format!("{}:{}", self.index(), self.fingerprint.display_prefix())
    }

    pub fn durable_locator(&self) -> StoredEntryLocatorV1 {
        StoredEntryLocatorV1 {
            version: ENTRY_FINGERPRINT_VERSION,
            index: self.index() as u64,
            digest: self.fingerprint.as_str().to_string(),
        }
    }
}

pub fn resolve_entry(
    graph: &LifeCoreList,
    state: &CliStateV1,
    locator: &EntryLocator,
    expectation: EntryExpectation,
    view_end: Option<usize>,
) -> Result<ResolvedEntry, AppError> {
    //A Keep this order aligned with SPEC.md: an index and digest establish
    //A identity before callers may learn kind or selected-view membership.
    let context = EntryIndex(locator.index()).before_and_including();
    let Some((entry, circumstance)) = graph.get_with_circumstance(context) else {
        return Err(AppError::unknown_entry_index(locator.index()));
    };
    let fingerprint = EntryFingerprint::for_entry(state, context.index(), &entry)?;

    if !fingerprint.as_str().starts_with(locator.prefix()) {
        return Err(AppError::entry_fingerprint_mismatch(
            locator.index(),
            locator.prefix(),
            fingerprint.display_prefix(),
        ));
    }

    if expectation == EntryExpectation::Node && !entry.is_node() {
        return Err(AppError::entry_not_node(locator.index()));
    }

    if let Some(end) = view_end
        && locator.index() >= end
    {
        return Err(AppError::entry_outside_view(locator.index(), end));
    }

    Ok(ResolvedEntry {
        entry,
        circumstance,
        fingerprint,
    })
}

pub fn resolve_index(
    graph: &LifeCoreList,
    state: &CliStateV1,
    index: usize,
    expectation: EntryExpectation,
    view_end: Option<usize>,
) -> Result<ResolvedEntry, AppError> {
    let context = EntryIndex(index).before_and_including();
    let entry = graph
        .get(context)
        .ok_or_else(|| AppError::unknown_entry_index(index))?;
    let fingerprint = EntryFingerprint::for_entry(state, context.index(), &entry)?;

    resolve_entry(
        graph,
        state,
        &EntryLocator {
            index,
            prefix: fingerprint.as_str().to_string(),
        },
        expectation,
        view_end,
    )
}

pub fn resolve_stored_locator(
    graph: &LifeCoreList,
    state: &CliStateV1,
    locator: &StoredEntryLocatorV1,
    expectation: EntryExpectation,
    view_end: Option<usize>,
) -> Result<ResolvedEntry, AppError> {
    let index = usize::try_from(locator.index)
        .map_err(|_| AppError::invalid_state("stored entry index overflows this platform"))?;
    resolve_entry(
        graph,
        state,
        &EntryLocator {
            index,
            prefix: locator.digest.clone(),
        },
        expectation,
        view_end,
    )
}
