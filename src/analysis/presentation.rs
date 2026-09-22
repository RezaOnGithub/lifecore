use super::{EvaluatedView, FindingKey, MessageType, Observation, Report};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Write,
    hash::{BuildHasher, RandomState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DismissalReason {
    Acknowledged,
    FalsePositive,
    NoLongerRelevant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismissal {
    pub key: FindingKey,
    pub reason: DismissalReason,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismissals {
    records: Vec<Dismissal>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("inconsistencies cannot be dismissed")]
pub struct CannotDismissInconsistency;

impl Dismissals {
    pub fn records(&self) -> impl Iterator<Item = &Dismissal> {
        self.records.iter()
    }
    pub fn get(&self, key: &FindingKey) -> Option<&Dismissal> {
        self.records.iter().find(|record| record.key == *key)
    }
    pub fn dismiss(
        &mut self,
        key: FindingKey,
        reason: DismissalReason,
        note: Option<String>,
    ) -> Result<(), CannotDismissInconsistency> {
        if key.kind == MessageType::Inconsistency {
            return Err(CannotDismissInconsistency);
        }
        if let Some(record) = self.records.iter_mut().find(|record| record.key == key) {
            record.reason = reason;
            record.note = note;
        } else {
            self.records.push(Dismissal { key, reason, note });
        }
        Ok(())
    }
    //A Restore is keyed by the stored record, without requiring an emitted finding.
    pub fn restore(&mut self, key: &FindingKey) -> bool {
        let before = self.records.len();
        self.records.retain(|record| record.key != *key);
        before != self.records.len()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReportSelection {
    #[default]
    Current,
    History,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdvisoryFilter {
    #[default]
    All,
    Warnings,
    Suggestions,
    None,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReportOptions {
    pub selection: ReportSelection,
    pub advisories: AdvisoryFilter,
    pub include_dismissed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindingGroup<'a> {
    pub key: &'a FindingKey,
    pub observations: Vec<&'a Observation>,
    pub selected: Vec<&'a Observation>,
    pub dismissal: Option<&'a Dismissal>,
}

impl Report {
    pub fn groups<'a>(
        &'a self,
        dismissals: &'a Dismissals,
        options: ReportOptions,
    ) -> Vec<FindingGroup<'a>> {
        self.groups_with_hasher(dismissals, options, RandomState::new())
    }

    pub(super) fn groups_with_hasher<'a, S: BuildHasher>(
        &'a self,
        dismissals: &'a Dismissals,
        options: ReportOptions,
        hasher: S,
    ) -> Vec<FindingGroup<'a>> {
        //A The map is only an index; output follows first observation order.
        //A HashMap compares complete keys even if hashes collide.
        let mut index = HashMap::<&FindingKey, usize, S>::with_hasher(hasher);
        let mut groups = Vec::<FindingGroup<'a>>::new();
        for observation in self.observations() {
            let key = &observation.key;
            let position = *index.entry(key).or_insert_with(|| {
                groups.push(FindingGroup {
                    key,
                    observations: vec![],
                    selected: vec![],
                    dismissal: dismissals.get(key),
                });
                groups.len() - 1
            });
            let group = &mut groups[position];
            group.observations.push(observation);
            let inconsistency = observation.kind.ty() == MessageType::Inconsistency;
            let category = match options.advisories {
            AdvisoryFilter::All => true,
            AdvisoryFilter::Warnings => observation.kind.ty() == MessageType::Warning,
            AdvisoryFilter::Suggestions => observation.kind.ty() == MessageType::Suggestion,
            AdvisoryFilter::None => false,
            };
            let boundary = options.selection == ReportSelection::History
                || observation.evaluated_view == EvaluatedView::Initialization
                || observation.evaluated_view == EvaluatedView::Prefix(self.view)
                || observation.retain_in_current_report;
            if inconsistency
                || (category
                    && boundary
                    && (options.include_dismissed || group.dismissal.is_none()))
            {
                group.selected.push(observation);
            }
        }
        groups.retain(|group| !group.selected.is_empty());
        groups
    }

    pub fn render(&self, dismissals: &Dismissals, options: ReportOptions) -> String {
        let mut text = format!(
            "{:?} report at boundary {} ({})\n",
            options.selection, self.view.end_exclusive, self.view.database
        );
        for group in self.groups(dismissals, options) {
            writeln!(
                text,
                "{:?} {} {:?}{}",
                group.key.kind,
                group.key.analyzer.name,
                group.key.analyzer_certificate.0,
                if group.dismissal.is_some() && group.key.kind != MessageType::Inconsistency {
                    " (dismissed)"
                } else {
                    ""
                }
            )
            .unwrap();
            for observation in group.selected {
                let at = match observation.evaluated_view {
                EvaluatedView::Initialization => "initialization".to_string(),
                EvaluatedView::Prefix(view) => format!("boundary {}", view.end_exclusive),
                };
                let retained = if observation.retain_in_current_report
                    && matches!(observation.evaluated_view, EvaluatedView::Prefix(view) if view != self.view)
                {
                    " (historical advisory; current applicability unconfirmed)"
                } else {
                    ""
                };
                writeln!(text, "  {at}{retained}: {}", observation.message).unwrap();
            }
        }
        text
    }
}
