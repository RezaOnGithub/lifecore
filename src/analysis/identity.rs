use crate::OffsetTimestamp;
use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::MessageType;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerIdentity {
    pub name: String,
    pub version: Version,
    // Preserve the caller's exact spelling even if the analyzer normalizes it.
    pub configuration: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationInputs {
    // The instant with the offset the host observed it at, so an analyzer
    // can find the local date without reading the system zone.
    pub evaluation_time: Option<OffsetTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnalyzerCertificate(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingKey {
    pub database: Uuid,
    pub analyzer: AnalyzerIdentity,
    pub kind: MessageType,
    // The analyzer owns this complete semantic identity; shared code never
    // parses it or substitutes a digest for exact equality.
    pub analyzer_certificate: AnalyzerCertificate,
}
