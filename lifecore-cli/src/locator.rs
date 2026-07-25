use std::{fmt, str::FromStr};

use anyhow::{Result, anyhow, bail};
use lifecore::{Context, Entry, EntryContext, EntryIndex, LifeCore, LifeCoreList, Node};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_512};
use uuid::Uuid;

use crate::state::StateFile;

const DISPLAY_DIGITS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableLocator {
    pub index: usize,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locator {
    pub index: usize,
    pub prefix: String,
}

impl FromStr for Locator {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.trim() != value {
            bail!("entry locator may not contain surrounding whitespace");
        }
        let (index, prefix) = value
            .split_once(':')
            .ok_or_else(|| anyhow!("entry locator must be INDEX:FINGERPRINT"))?;
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("entry locator index must contain only ASCII decimal digits");
        }
        if !(5..=128).contains(&prefix.len())
            || !prefix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("entry fingerprint must be 5 to 128 lowercase hexadecimal digits");
        }
        Ok(Self {
            index: index.parse().map_err(|_| anyhow!("entry index overflow"))?,
            prefix: prefix.to_string(),
        })
    }
}

impl fmt::Display for DurableLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}",
            self.index,
            &self.digest[..DISPLAY_DIGITS.min(self.digest.len())]
        )
    }
}

pub fn fingerprint(uuid: Uuid, lifecore: &LifeCoreList, index: usize) -> Result<String> {
    let context = EntryIndex(index).before_and_including();
    let entry = lifecore
        .get(context)
        .ok_or_else(|| anyhow!("unknown entry index {index}"))?;
    let mut input = Vec::new();
    bytes(&mut input, b"lifecore-entry-fingerprint");
    input.extend_from_slice(&1u16.to_be_bytes());
    input.extend_from_slice(uuid.as_bytes());
    input.extend_from_slice(&(index as u64).to_be_bytes());
    match entry {
    Entry::Node(node) => {
        input.push(1);
        bytes(&mut input, node.ty_name.as_bytes());
        input.extend_from_slice(&(node.field.len() as u64).to_be_bytes());
    }
    Entry::Edge(edge) => {
        input.push(2);
        bytes(&mut input, edge.ty_name.as_bytes());
    }
    }
    Ok(format!("{:x}", Sha3_512::digest(input)))
}

fn bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

pub fn durable(uuid: Uuid, lifecore: &LifeCoreList, index: usize) -> Result<DurableLocator> {
    Ok(DurableLocator {
        index,
        digest: fingerprint(uuid, lifecore, index)?,
    })
}

pub fn resolve(
    uuid: Uuid,
    lifecore: &LifeCoreList,
    locator: &Locator,
    view: Context,
) -> Result<(Entry, DurableLocator)> {
    let digest = fingerprint(uuid, lifecore, locator.index)?;
    if !digest.starts_with(&locator.prefix) {
        bail!(
            "entry fingerprint mismatch: supplied {}, actual {}",
            locator.prefix,
            &digest[..DISPLAY_DIGITS]
        );
    }
    let context = context_in_view(lifecore, locator.index, view)
        .ok_or_else(|| anyhow!("entry is outside the selected view"))?;
    let entry = lifecore
        .get(context)
        .ok_or_else(|| anyhow!("unknown entry index {}", locator.index))?;
    Ok((
        entry,
        DurableLocator {
            index: locator.index,
            digest,
        },
    ))
}

pub fn resolve_durable(
    uuid: Uuid,
    lifecore: &LifeCoreList,
    locator: &DurableLocator,
    view: Context,
) -> Result<(Entry, DurableLocator)> {
    resolve(
        uuid,
        lifecore,
        &Locator {
            index: locator.index,
            prefix: locator.digest.clone(),
        },
        view,
    )
}

pub fn resolve_node(state: &StateFile, locator: &DurableLocator, view: Context) -> Result<Node> {
    match resolve_durable(state.cli.state_uuid, &state.lifecore, locator, view)?.0 {
    Entry::Node(node) => Ok(node),
    Entry::Edge(_) => bail!("entry is not a node"),
    }
}

pub fn context_in_view(
    lifecore: &LifeCoreList,
    index: usize,
    view: Context,
) -> Option<EntryContext> {
    lifecore
        .window(view.items())
        .find(|entry| entry.entry_context().entry() == EntryIndex(index))
        .map(|entry| entry.entry_context())
}

pub fn latest_node(lifecore: &LifeCoreList, view: Context) -> Option<Node> {
    lifecore.nodes(view).last()
}

pub fn validate_alias(value: &str) -> Result<()> {
    if value == "HEAD" {
        bail!("HEAD is reserved and cannot be used as an alias");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("alias may not be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        bail!("alias must match [A-Za-z_][A-Za-z0-9_]*");
    }
    Ok(())
}
