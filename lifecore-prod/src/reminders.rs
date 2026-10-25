//! Planning's two nonsticky messages, re-emitted at every prefix while their
//! condition holds: the bootstrap suggestion (PLANNING §8.6) and the opt-in
//! stale-review nudge (§10).

use crate::{
    findings::PlanningMessage,
    payload::{self, PayloadError},
    vocabulary::PlanningType,
    windows::Reviews,
};
use jiff::SignedDuration;
use lifecore::{Timestamp, View};

// One suggestion per canonical declaration whose name resolves as missing.
// An amended declaration is never suggested back to its canonical form;
// section 3.4 judges it instead (decision 36).
pub(crate) fn bootstrap(view: View<'_>) -> Vec<PlanningMessage> {
    let declarations = view.declarations();
    PlanningType::ALL
        .into_iter()
        .filter(|ty| declarations.is_missing(ty.name()))
        .map(|name| PlanningMessage::Bootstrap { name })
        .collect()
}

// Once the view holds an item, a warning when no review exists or the latest
// review's `at` is more than `days` days before `now`.
pub(crate) fn stale_review(
    view: View<'_>,
    days: u64,
    now: Timestamp,
) -> Result<Option<PlanningMessage>, PayloadError> {
    let holds_item = view
        .fresh_nodes()
        .any(|node| PlanningType::from_name(&node.ty_name).is_some_and(PlanningType::is_item));
    if !holds_item {
        return Ok(None);
    }
    let Some(latest) = Reviews::new(view).latest() else {
        return Ok(Some(PlanningMessage::StaleReview { review: None }));
    };
    let age = now.duration_since(payload::timestamp(latest, "at")?);
    // A threshold too large to represent is never exceeded.
    let threshold = i64::try_from(days)
        .ok()
        .and_then(|days| days.checked_mul(24))
        .and_then(SignedDuration::try_from_hours)
        .unwrap_or(SignedDuration::MAX);
    Ok((age > threshold).then_some(PlanningMessage::StaleReview {
        review: Some(latest.id),
    }))
}
