use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use lifecore::{NodeBodyExt, PayloadExt};
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
    pub body: BTreeMap<String, toml::Value>,
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
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read sidecar {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("failed to deserialize sidecar {}", path.display()))
    }
}

impl SidecarEntry {
    pub fn node_body(&self) -> Result<NodeBodyExt> {
        let mut body = BTreeMap::new();
        for (name, value) in &self.body {
            let payload = match value {
            toml::Value::String(value) => PayloadExt::Text(value.clone()),
            toml::Value::Integer(value) => PayloadExt::Number(*value),
            unsupported => bail!(
                "sidecar field {name:?} uses unsupported TOML value {unsupported}; only strings and integers are supported"
            ),
            };
            body.insert(name.clone(), payload);
        }
        Ok(NodeBodyExt(body))
    }
}

#[cfg(test)]
mod tests {
    use lifecore::PayloadExt;

    use super::*;

    #[test]
    fn parses_documented_entry_shape() {
        let sidecar: Sidecar = toml::from_str(
            r#"
                [[entry]]
                alias = "BuyMilk"
                ty = "Note"
                parent = "P"
                [entry.body]
                content = "Buy Milk"
                priority = 2
                [entry.connect]
                connect_to = [{ target = "AtWork", ty = "Related" }]
                connect_back_from = [{ target = "Dairy", ty = "Accumulate" }]
            "#,
        )
        .unwrap();

        assert_eq!(sidecar.entries.len(), 1);
        let entry = &sidecar.entries[0];
        assert_eq!(
            entry.node_body().unwrap().0["content"],
            PayloadExt::Text("Buy Milk".into())
        );
        assert_eq!(
            entry.node_body().unwrap().0["priority"],
            PayloadExt::Number(2)
        );
        assert_eq!(entry.connect.connect_to[0].target, "AtWork");
        assert_eq!(entry.connect.connect_back_from[0].target, "Dairy");
    }

    #[test]
    fn rejects_unsupported_payload_types() {
        let sidecar: Sidecar = toml::from_str(
            r#"
                [[entry]]
                ty = "Thing"
                [entry.body]
                value = true
            "#,
        )
        .unwrap();
        assert!(sidecar.entries[0].node_body().is_err());
    }
}
