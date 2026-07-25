use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Sidecar {
    #[serde(default, rename = "entry")]
    pub entries: Vec<SidecarEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SidecarEntry {
    pub ty: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub body: toml::Table,
    #[serde(default)]
    pub connect: SidecarConnections,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SidecarConnections {
    #[serde(default)]
    pub connect_to: Vec<SidecarConnection>,
    #[serde(default)]
    pub connect_back_from: Vec<SidecarConnection>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SidecarConnection {
    pub target: String,
    pub ty: String,
}

impl Sidecar {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read sidecar {}", path.display()))?;
        toml::from_str(&source)
            .with_context(|| format!("failed to parse sidecar {}", path.display()))
    }
}
