//! Section 3.4 judged at the prefix of a declaration of a planning type: a
//! user may amend planning's declarations as long as they keep what the
//! rules read (decision 36).

use crate::{
    findings::{FieldRule, PlanningMessage, Requirement},
    vocabulary::{Label, PlanningType},
};
use lifecore::{
    Node,
    typing::{FieldStatus, Type, ValueType},
};

// Judges `node`, a `Type` node that ends its view. A declaration that does
// not parse is TypeCheck's to report, and one that claims another name is
// not planning's.
pub(crate) fn judge(node: &Node) -> Vec<PlanningMessage> {
    let Ok(declared) = Type::from_body(&node.payload) else {
        return Vec::new();
    };
    let Some(name) = PlanningType::from_name(&declared.name) else {
        return Vec::new();
    };
    unmet(name, &declared)
        .into_iter()
        .map(|requirement| PlanningMessage::UnmetRequirement {
            declaration: node.id,
            name,
            requirement,
        })
        .collect()
}

// Every requirement of the §3.4 table that `declared` does not meet.
pub(crate) fn unmet(name: PlanningType, declared: &Type) -> Vec<Requirement> {
    let canonical = name.canonical();
    let mut unmet = Vec::new();
    for (permitted, canonical, outgoing) in [
        (&declared.outgoing, &canonical.outgoing, true),
        (&declared.incoming, &canonical.incoming, false),
    ] {
        for label in canonical.difference(permitted) {
            let label = Label::from_name(label).expect("a canonical label");
            unmet.push(Requirement::PermitsLabel { label, outgoing });
        }
    }
    if !name.is_item() && !declared.successors.is_empty() {
        unmet.push(Requirement::NoSuccessors);
    }
    let fields: &[(&'static str, FieldRule)] = match name {
    PlanningType::Stance => &[
        ("kind", FieldRule::MandatoryText),
        ("cost", FieldRule::OptionalInteger),
        ("horizon", FieldRule::OptionalInteger),
    ],
    PlanningType::Review => &[
        ("budget", FieldRule::MandatoryInteger),
        ("at", FieldRule::MandatoryTimestamp),
    ],
    PlanningType::Concern | PlanningType::Habit => &[("quota", FieldRule::MandatoryInteger)],
    PlanningType::Goal | PlanningType::Project => &[("deadline", FieldRule::TimestampIfDeclared)],
    _ => &[],
    };
    for &(field, rule) in fields {
        let declaration = declared
            .fields
            .get(field)
            .map(|field| (field.status, &field.shape));
        let met = match (rule, declaration) {
        (FieldRule::MandatoryText, Some((FieldStatus::Mandatory, ValueType::Text)))
        | (FieldRule::MandatoryInteger, Some((FieldStatus::Mandatory, ValueType::Integer)))
        | (FieldRule::MandatoryTimestamp, Some((FieldStatus::Mandatory, ValueType::Timestamp)))
        | (FieldRule::TimestampIfDeclared, None | Some((_, ValueType::Timestamp))) => true,
        // A mandatory `cost` or `horizon` would demand it on the kinds that
        // forbid it (§3.3).
        (FieldRule::OptionalInteger, Some((status, ValueType::Integer))) => {
            status != FieldStatus::Mandatory
        }
        _ => false,
        };
        if !met {
            unmet.push(Requirement::Field { field, rule });
        }
    }
    unmet
}
