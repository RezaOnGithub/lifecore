//! Analyzer configuration (PLANNING §11).

use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningConfig {
    // Caps a `deferred` stance's `horizon` at its own prefix; `None` removes
    // the cap. An omitted key keeps the default, while `null` is no cap.
    #[serde(default = "default_cap")]
    pub max_deferral_horizon: Option<NonZeroU64>,
    // Enables the stale-review nudge, which reads the evaluation time.
    #[serde(default)]
    pub stale_review_after_days: Option<u64>,
    // An entry position; seals before it are not judged under §8.4 and §8.5.
    #[serde(default)]
    pub first_sealed_review: Option<usize>,
}

fn default_cap() -> Option<NonZeroU64> {
    NonZeroU64::new(12)
}

impl Default for PlanningConfig {
    fn default() -> Self {
        Self {
            max_deferral_horizon: default_cap(),
            stale_review_after_days: None,
            first_sealed_review: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlanningConfigError {
    #[error("invalid JSON configuration: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the configuration must be a JSON object")]
    NotAnObject,
}

impl PlanningConfig {
    // The empty string means the defaults; anything else must be a strict
    // JSON object. The derived parse rejects unknown and repeated fields,
    // fractions, and negatives, but would also accept an array in place of
    // the object, so the text's shape is checked separately.
    pub fn from_json(value: &str) -> Result<Self, PlanningConfigError> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        let config = serde_json::from_str(value)?;
        if !serde_json::from_str::<serde_json::Value>(value)?.is_object() {
            return Err(PlanningConfigError::NotAnObject);
        }
        Ok(config)
    }
}
