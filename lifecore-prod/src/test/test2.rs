use crate::{
    Completion, FieldRule, ItemState, Label, Planning, PlanningConfig, PlanningFactory,
    PlanningMessage::{self, *},
    PlanningType, Reason, Requirement, StanceKind, Superseded, ValuationOptions,
    situation::Situation,
    valuation,
};
use lifecore::{
    Body, EntryId, LifeCoreError, LifeCoreList, Payload, Timestamp, View,
    analysis::{
        AnalysisOutput, AnalysisRunner, AnalyzerFactory, EvaluatedView, EvaluationInputs,
        MessageKind, MessageType, TypedAnalyzer,
        presentation::{Dismissals, ReportOptions},
        suggestion::prepare_suggestion,
    },
    transaction::{
        self, Change, Checked, EntryChange, NewEdge, NewNode,
        Reference::{self, Added, Existing},
        RevisionIntent, prepare,
    },
    typing::Type,
};
use lifecore_typecheck::TypeCheckFactory;
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Scaffolding

fn runner_with(inputs: EvaluationInputs) -> AnalysisRunner {
    let mut runner = AnalysisRunner::new(inputs);
    runner
        .register_analyzer_kind(Box::new(PlanningFactory))
        .unwrap();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    runner
}
fn runner() -> AnalysisRunner {
    runner_with(EvaluationInputs::default())
}
fn configure(name: &str, configuration: &str) -> Change {
    Change::AddAnalyzer {
        name: name.into(),
        configuration: configuration.into(),
    }
}
fn declare(ty: &Type) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: Type::TYPE_NAME.into(),
        predecessor: None,
        intent: None,
        payload: ty.to_body(),
    }))
}
fn revise_declaration(ty: &Type, of: EntryId) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: Type::TYPE_NAME.into(),
        predecessor: Some(Existing(of)),
        intent: Some(RevisionIntent::Revise),
        payload: ty.to_body(),
    }))
}

// 2026-01-01T00:00:00Z.
const START: i64 = 1_767_225_600;
const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(START + seconds).unwrap()
}

struct Db {
    graph: LifeCoreList,
    // Hands out increasing `at` values, so that no two stances or events
    // collide under the core's duplicate rule.
    clock: i64,
}

impl Db {
    // The ten canonical declarations under an empty suite, so that any
    // structurally valid plan commits and planning is judged by calling the
    // analyzer on prefixes.
    fn new() -> Self {
        Self::with_suite(&[])
    }
    // No declarations and no analyzers.
    fn empty() -> Self {
        Self {
            graph: LifeCoreList::default(),
            clock: 0,
        }
    }
    // TypeCheck and Planning with `configuration`, as a planning database
    // configures them.
    fn planned(configuration: &str) -> Self {
        Self::with_suite(&[
            configure(TypeCheckFactory::name(), ""),
            configure(PlanningFactory::name(), configuration),
        ])
    }
    fn with_suite(suite: &[Change]) -> Self {
        let mut db = Self::empty();
        let mut changes = suite.to_vec();
        changes.extend(PlanningType::ALL.map(|ty| declare(&ty.canonical())));
        db.apply(&changes);
        db
    }
    fn view(&self) -> View<'_> {
        self.graph.view()
    }
    // A copy to try an alternative history on.
    fn fork(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            clock: self.clock,
        }
    }
    fn check(&self, changes: &[Change]) -> Checked {
        prepare(&self.graph, changes, &mut runner()).unwrap()
    }
    fn apply(&mut self, changes: &[Change]) -> Vec<EntryId> {
        match self.check(changes) {
        Checked::Accepted(plan) => {
            let ids = plan.added();
            plan.commit(&mut self.graph).unwrap();
            ids
        }
        Checked::Rejected(rejection) => panic!(
            "rejected:\n{}",
            rejection
                .analysis()
                .render(&Dismissals::default(), ReportOptions::default())
        ),
        }
    }
    fn one(&mut self, change: Change) -> EntryId {
        let [id] = self.apply(&[change]).try_into().unwrap();
        id
    }
    fn two(&mut self, changes: [Change; 2]) -> (EntryId, EntryId) {
        let [a, b] = self.apply(&changes).try_into().unwrap();
        (a, b)
    }
    fn tick(&mut self) -> Payload {
        self.clock += 60;
        at(self.clock).into()
    }

    // A root stance: `kind` with the integer fields given.
    fn stance(&mut self, kind: &str, fields: &[(&str, i64)]) -> Change {
        self.stance_node(kind, fields, None)
    }
    fn supersede(&mut self, of: EntryId, kind: &str, fields: &[(&str, i64)]) -> Change {
        self.stance_node(kind, fields, Some(of))
    }
    fn stance_node(&mut self, kind: &str, fields: &[(&str, i64)], of: Option<EntryId>) -> Change {
        let mut payload = Body::from([("kind".into(), kind.into()), ("at".into(), self.tick())]);
        for (name, value) in fields {
            payload.insert((*name).into(), (*value).into());
        }
        node_change(
            "Stance",
            payload,
            of.map(|of| (Existing(of), RevisionIntent::Revise)),
        )
    }
    // A stance and its `About` edge, as one plan.
    fn decide(&mut self, kind: &str, fields: &[(&str, i64)], about: EntryId) -> (EntryId, EntryId) {
        let stance = self.stance(kind, fields);
        self.two([stance, edge("About", Added(0), Existing(about))])
    }
    // A successor stance and its `About` edge, as one plan.
    fn redecide(
        &mut self,
        of: EntryId,
        kind: &str,
        fields: &[(&str, i64)],
        about: EntryId,
    ) -> (EntryId, EntryId) {
        let stance = self.supersede(of, kind, fields);
        self.two([stance, edge("About", Added(0), Existing(about))])
    }
    fn commit(&mut self, cost: i64, task: EntryId) -> (EntryId, EntryId) {
        self.decide("committed", &[("cost", cost)], task)
    }
    fn defer(&mut self, horizon: i64, item: EntryId) -> (EntryId, EntryId) {
        self.decide("deferred", &[("horizon", horizon)], item)
    }
    fn event(&mut self, ty: &str) -> Change {
        let payload = Body::from([("at".into(), self.tick())]);
        node_change(ty, payload, None)
    }
    // A `Done` and its `About` edge.
    fn finish(&mut self, item: EntryId) -> (EntryId, EntryId) {
        let done = self.event("Done");
        self.two([done, edge("About", Added(0), Existing(item))])
    }
    // A `Did` and its `About` edge.
    fn attend(&mut self, item: EntryId) -> (EntryId, EntryId) {
        let did = self.event("Did");
        self.two([did, edge("About", Added(0), Existing(item))])
    }
    fn review_change(&mut self, budget: i64) -> Change {
        // Reviews a week apart, so that deadline estimates have a median.
        let week = (self.reviews() as i64 + 1) * 7 * DAY;
        review_at(budget, at(week))
    }
    fn review(&mut self, budget: i64) -> EntryId {
        let review = self.review_change(budget);
        self.one(review)
    }
    fn reviews(&self) -> usize {
        self.view()
            .nodes()
            .filter(|node| node.ty_name == "Review")
            .count()
    }
    fn link(&mut self, label: &str, from: EntryId, to: EntryId) -> EntryId {
        self.one(edge(label, Existing(from), Existing(to)))
    }
}

fn node_change(
    ty: &str,
    payload: Body,
    predecessor: Option<(Reference, RevisionIntent)>,
) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: ty.into(),
        predecessor: predecessor.map(|(reference, _)| reference),
        intent: predecessor.map(|(_, intent)| intent),
        payload,
    }))
}
fn titled(title: &str, quota: Option<i64>) -> Body {
    let mut payload = Body::from([("title".into(), title.into())]);
    if let Some(quota) = quota {
        payload.insert("quota".into(), quota.into());
    }
    payload
}
fn item(ty: &str, title: &str) -> Change {
    let quota = matches!(ty, "Habit" | "Concern").then_some(1);
    node_change(ty, titled(title, quota), None)
}
fn habit(title: &str, quota: i64) -> Change {
    node_change("Habit", titled(title, Some(quota)), None)
}
fn concern(title: &str, quota: i64) -> Change {
    node_change("Concern", titled(title, Some(quota)), None)
}
fn revise(ty: &str, title: &str, of: EntryId) -> Change {
    let quota = matches!(ty, "Habit" | "Concern").then_some(1);
    node_change(
        ty,
        titled(title, quota),
        Some((Existing(of), RevisionIntent::Revise)),
    )
}
fn branch(ty: &str, title: &str, of: EntryId) -> Change {
    let quota = matches!(ty, "Habit" | "Concern").then_some(1);
    node_change(
        ty,
        titled(title, quota),
        Some((Existing(of), RevisionIntent::Branch)),
    )
}
fn review_at(budget: i64, at: Timestamp) -> Change {
    let payload = Body::from([("budget".into(), budget.into()), ("at".into(), at.into())]);
    node_change("Review", payload, None)
}
fn edge(label: &str, from: Reference, to: Reference) -> Change {
    Change::Entry(EntryChange::Edge(NewEdge {
        ty_name: label.into(),
        from,
        to,
    }))
}

fn planning(configuration: &str) -> Planning {
    Planning::new(PlanningConfig::from_json(configuration).unwrap())
}
// The typed findings at the prefix whose newest entry is `newest`, other
// than bootstrap's, under the default configuration.
fn judged(db: &Db, newest: EntryId) -> Vec<PlanningMessage> {
    judged_with(db, newest, "")
}
fn judged_with(db: &Db, newest: EntryId, configuration: &str) -> Vec<PlanningMessage> {
    at_prefix(db, newest.position() + 1, configuration)
}
// The typed findings at the prefix that ends just before `newest`.
fn judged_before(db: &Db, newest: EntryId) -> Vec<PlanningMessage> {
    at_prefix(db, newest.position(), "")
}
fn at_prefix(db: &Db, end_exclusive: usize, configuration: &str) -> Vec<PlanningMessage> {
    planning(configuration)
        .analyze_inner(
            db.view().prefix(end_exclusive).unwrap(),
            &EvaluationInputs::default(),
        )
        .unwrap()
        .into_iter()
        .filter(|finding| !matches!(finding, Bootstrap { .. }))
        .collect()
}
// The judgment at `newest` holds `finding`, and the prefix before does not.
fn appears_at(db: &Db, newest: EntryId, finding: &PlanningMessage) {
    assert!(
        judged(db, newest).contains(finding),
        "{finding:?} missing at {newest:?}: {:?}",
        judged(db, newest)
    );
    assert!(!judged_before(db, newest).contains(finding));
}
fn quiet(db: &Db, newest: EntryId) {
    assert_eq!(judged(db, newest), vec![], "at {newest:?}");
}

fn certificate(finding: &PlanningMessage) -> String {
    serde_json::to_string(finding).unwrap()
}
fn planning_keys(output: &AnalysisOutput, ty: MessageType) -> BTreeSet<String> {
    output
        .observations()
        .filter(|o| o.key.analyzer.name == PlanningFactory::name() && o.key.kind == ty)
        .map(|o| o.key.analyzer_certificate.0.clone())
        .collect()
}
// A check the suite rejects, with the provisional identities of its
// operations.
fn rejected(checked: Checked) -> (Vec<EntryId>, AnalysisOutput) {
    let Checked::Rejected(rejection) = checked else {
        panic!("expected a rejection");
    };
    let operations = rejection.operations().iter().flatten().copied().collect();
    (operations, rejection.analysis().clone())
}
fn certificates(findings: &[PlanningMessage]) -> BTreeSet<String> {
    findings.iter().map(certificate).collect()
}
fn inconsistencies(output: &AnalysisOutput) -> BTreeSet<String> {
    planning_keys(output, MessageType::Inconsistency)
}
fn warnings(output: &AnalysisOutput) -> BTreeSet<String> {
    planning_keys(output, MessageType::Warning)
}

fn open_window(db: &Db) -> Situation<'_> {
    Situation::in_open_window(db.view()).unwrap()
}
fn in_open_window_at(db: &Db, newest: EntryId) -> Situation<'_> {
    Situation::in_open_window(db.view().prefix(newest.position() + 1).unwrap()).unwrap()
}
fn sealed_at(db: &Db, review: EntryId) -> Situation<'_> {
    Situation::at_seal(db.view().prefix(review.position() + 1).unwrap()).unwrap()
}

// The valuation at the head, as (revision, tier, reasons).
fn ranking(db: &Db) -> Vec<(EntryId, u8, Vec<Reason>)> {
    ranking_with(db, &ValuationOptions::default())
}
fn ranking_with(db: &Db, options: &ValuationOptions) -> Vec<(EntryId, u8, Vec<Reason>)> {
    valuation(db.view(), options)
        .unwrap()
        .ranked
        .into_iter()
        .map(|row| {
            let reasons = row.reasons.into_iter().map(|r| r.reason).collect();
            (row.revision, row.tier, reasons)
        })
        .collect()
}
fn row(db: &Db, revision: EntryId) -> Option<(u8, Vec<Reason>)> {
    ranking(db)
        .into_iter()
        .find(|(id, _, _)| *id == revision)
        .map(|(_, tier, reasons)| (tier, reasons))
}

// The current declaration claiming `ty`.
fn declaration(db: &Db, ty: PlanningType) -> EntryId {
    db.view()
        .declarations()
        .declared(ty.name())
        .expect("a declared planning type")
        .0
}
fn labels(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Vocabulary, configuration, declarations, and bootstrap

// Acceptance test 1.
#[test]
fn canonical_declarations_round_trip_and_match_the_endpoint_table() {
    for ty in PlanningType::ALL {
        let canonical = ty.canonical();
        assert_eq!(canonical.name, ty.name());
        assert_eq!(Type::from_body(&canonical.to_body()).unwrap(), canonical);
        assert_eq!(crate::declarations::unmet(ty, &canonical), vec![]);
        assert!(canonical.successors.is_empty());
    }
    // Section 11's three, field by field.
    let task = PlanningType::Task.canonical();
    assert_eq!(task.outgoing, labels(&["Enables", "Helps"]));
    assert_eq!(
        task.incoming,
        labels(&["Enables", "Helps", "About", "Until"])
    );
    assert_eq!(
        task.fields.keys().collect::<Vec<_>>(),
        ["note", "title"].iter().collect::<Vec<_>>()
    );
    let stance = PlanningType::Stance.canonical();
    assert_eq!(stance.outgoing, labels(&["About", "Until"]));
    assert_eq!(stance.fields.len(), 5);
    let review = PlanningType::Review.canonical();
    assert!(review.outgoing.is_empty() && review.incoming.is_empty());
    // A label is declared at an end exactly when section 3.2 permits it there
    // for some type at the other end.
    for ty in PlanningType::ALL {
        let canonical = ty.canonical();
        for label in Label::ALL {
            let as_source = PlanningType::ALL.iter().any(|&to| label.permits(ty, to));
            let as_target = PlanningType::ALL
                .iter()
                .any(|&from| label.permits(from, ty));
            assert_eq!(
                canonical.outgoing.contains(label.name()),
                as_source,
                "{ty} {label}"
            );
            assert_eq!(
                canonical.incoming.contains(label.name()),
                as_target,
                "{ty} {label}"
            );
        }
    }
}

// Acceptance test 1.
#[test]
fn bootstrap_suggests_each_missing_declaration_and_never_an_amended_one() {
    let mut db = Db::empty();
    let all = PlanningType::ALL.map(|name| Bootstrap { name }).to_vec();
    let at_head = |db: &Db| {
        planning("")
            .analyze_inner(db.view(), &EvaluationInputs::default())
            .unwrap()
    };
    assert_eq!(at_head(&db), all);
    // An amended `Task` declaration claims the name, so it is not suggested
    // back to its canonical form.
    let mut task = PlanningType::Task.canonical();
    task.outgoing.insert("Cites".into());
    db.apply(&[declare(&PlanningType::Goal.canonical()), declare(&task)]);
    assert_eq!(
        at_head(&db),
        all[1..3]
            .iter()
            .chain(&all[4..])
            .cloned()
            .collect::<Vec<_>>()
    );

    // Through the runner, each suggestion's action is its canonical
    // declaration, and applying one quiets it.
    let mut db = Db::with_suite(&[configure(PlanningFactory::name(), "")]);
    db.graph = LifeCoreList::default();
    db.apply(&[configure(PlanningFactory::name(), "")]);
    let output = runner().run(&db.graph).unwrap();
    let suggestions = output.suggestions().collect::<Vec<_>>();
    assert_eq!(suggestions.len(), 10);
    for (observation, ty) in suggestions.iter().zip(PlanningType::ALL) {
        assert!(!observation.diagnostic.sticky);
        assert!(observation.diagnostic.subjects.is_empty());
        let MessageKind::Suggestion(actions) = &observation.diagnostic.kind else {
            panic!("a suggestion");
        };
        assert_eq!(
            actions,
            &vec![match declare(&ty.canonical()) {
            Change::Entry(change) => change,
            Change::AddAnalyzer { .. } => unreachable!(),
            }]
        );
    }
    let plan = prepare_suggestion(&db.graph, suggestions[3], &mut runner())
        .unwrap()
        .accepted()
        .unwrap();
    plan.commit(&mut db.graph).unwrap();
    assert!(!at_head(&db).contains(&Bootstrap {
        name: PlanningType::Task
    }));
    assert_eq!(at_head(&db).len(), 9);
}

// Acceptance test 16.
#[test]
fn a_declaration_amendment_is_judged_against_section_3_4_at_its_own_prefix() {
    let mut db = Db::new();
    let unmet = |declaration, name, requirement| UnmetRequirement {
        declaration,
        name,
        requirement,
    };
    // Fields and labels of the user's own, removed again, and a deprecated
    // `title`, which planning does not read.
    let mut task = PlanningType::Task.canonical();
    task.fields.insert(
        "place".into(),
        lifecore::typing::FieldDecl {
            status: lifecore::typing::FieldStatus::Optional,
            shape: lifecore::typing::ValueType::Text,
        },
    );
    task.outgoing.insert("Cites".into());
    let amended = db.one(revise_declaration(
        &task,
        declaration(&db, PlanningType::Task),
    ));
    quiet(&db, amended);
    let mut task = PlanningType::Task.canonical();
    task.fields.get_mut("title").unwrap().status = lifecore::typing::FieldStatus::Deprecated;
    let deprecated = db.one(revise_declaration(&task, amended));
    quiet(&db, deprecated);

    // What the rules read cannot go.
    let mut review = PlanningType::Review.canonical();
    review.fields.remove("budget");
    let no_budget = db.one(revise_declaration(
        &review,
        declaration(&db, PlanningType::Review),
    ));
    let requirement = Requirement::Field {
        field: "budget",
        rule: FieldRule::MandatoryInteger,
    };
    assert_eq!(
        judged(&db, no_budget),
        vec![unmet(no_budget, PlanningType::Review, requirement)]
    );
    assert_eq!(judged_before(&db, no_budget), vec![]);

    let mut stance = PlanningType::Stance.canonical();
    stance.fields.get_mut("kind").unwrap().status = lifecore::typing::FieldStatus::Optional;
    stance.fields.get_mut("cost").unwrap().status = lifecore::typing::FieldStatus::Mandatory;
    stance.fields.remove("horizon");
    let loose = db.one(revise_declaration(
        &stance,
        declaration(&db, PlanningType::Stance),
    ));
    let field = |field, rule| Requirement::Field { field, rule };
    assert_eq!(
        judged(&db, loose),
        vec![
            unmet(
                loose,
                PlanningType::Stance,
                field("kind", FieldRule::MandatoryText)
            ),
            unmet(
                loose,
                PlanningType::Stance,
                field("cost", FieldRule::OptionalInteger)
            ),
            unmet(
                loose,
                PlanningType::Stance,
                field("horizon", FieldRule::OptionalInteger)
            ),
        ]
    );

    let mut task = PlanningType::Task.canonical();
    task.incoming.remove("About");
    let deaf = db.one(revise_declaration(&task, deprecated));
    assert_eq!(
        judged(&db, deaf),
        vec![unmet(
            deaf,
            PlanningType::Task,
            Requirement::PermitsLabel {
                label: Label::About,
                outgoing: false,
            }
        )]
    );

    let mut review = PlanningType::Review.canonical();
    review.successors.insert("Note".into());
    let replaced = db.one(revise_declaration(&review, no_budget));
    assert_eq!(
        judged(&db, replaced),
        vec![unmet(
            replaced,
            PlanningType::Review,
            Requirement::NoSuccessors
        )]
    );

    // `deadline` may go, or change policy, but not type; quotas stay.
    let mut goal = PlanningType::Goal.canonical();
    goal.fields.remove("deadline");
    let undated = db.one(revise_declaration(
        &goal,
        declaration(&db, PlanningType::Goal),
    ));
    quiet(&db, undated);
    let mut goal = PlanningType::Goal.canonical();
    goal.fields.get_mut("deadline").unwrap().shape = lifecore::typing::ValueType::Text;
    let text_deadline = db.one(revise_declaration(&goal, undated));
    assert_eq!(
        judged(&db, text_deadline),
        vec![unmet(
            text_deadline,
            PlanningType::Goal,
            field("deadline", FieldRule::TimestampIfDeclared)
        )]
    );
    let mut habit = PlanningType::Habit.canonical();
    habit.fields.get_mut("quota").unwrap().status = lifecore::typing::FieldStatus::Encouraged;
    let lax = db.one(revise_declaration(
        &habit,
        declaration(&db, PlanningType::Habit),
    ));
    assert_eq!(
        judged(&db, lax),
        vec![unmet(
            lax,
            PlanningType::Habit,
            field("quota", FieldRule::MandatoryInteger)
        )]
    );

    // A declaration of another name, or one that does not parse, is not
    // planning's to judge.
    let note = db.one(declare(&Type::new("Note", &[], &["About"], &[], &[])));
    quiet(&db, note);
    let malformed = db.one(node_change(
        Type::TYPE_NAME,
        Body::from([("name".into(), "Task".into()), ("fields".into(), 3.into())]),
        None,
    ));
    quiet(&db, malformed);
}

// Acceptance test 25, the configuration half.
#[test]
fn configuration_is_empty_or_a_strict_json_object() {
    let defaults = PlanningConfig::from_json("").unwrap();
    assert_eq!(defaults, PlanningConfig::default());
    assert_eq!(defaults.max_deferral_horizon.unwrap().get(), 12);
    assert_eq!(defaults.stale_review_after_days, None);
    assert_eq!(defaults.first_sealed_review, None);
    assert_eq!(PlanningConfig::from_json("{}").unwrap(), defaults);
    let uncapped = PlanningConfig::from_json(r#"{"max_deferral_horizon": null}"#).unwrap();
    assert_eq!(uncapped.max_deferral_horizon, None);
    let full = PlanningConfig::from_json(
        r#"{"max_deferral_horizon": 4, "stale_review_after_days": 0, "first_sealed_review": 30}"#,
    )
    .unwrap();
    assert_eq!(full.max_deferral_horizon.unwrap().get(), 4);
    assert_eq!(full.stale_review_after_days, Some(0));
    assert_eq!(full.first_sealed_review, Some(30));
    for rejected in [
        " ",
        "null",
        "[12, null, null]",
        r#"{"horizon_windows": 6}"#,
        r#"{"max_deferral_horizon": 0}"#,
        r#"{"max_deferral_horizon": 1.5}"#,
        r#"{"max_deferral_horizon": -1}"#,
        r#"{"stale_review_after_days": -1}"#,
        r#"{"first_sealed_review": "30"}"#,
        r#"{"max_deferral_horizon": 3, "max_deferral_horizon": 4}"#,
    ] {
        assert!(PlanningConfig::from_json(rejected).is_err(), "{rejected}");
        assert!(matches!(
            PlanningFactory.analyzer_with_config(rejected),
            Err(LifeCoreError::InvalidAnalyzerConfiguration { .. })
        ));
    }
}

// ---------------------------------------------------------------------------
// Section 8.1: at a node's own prefix

// Acceptance test 26, the node half.
#[test]
fn kind_and_conditional_field_rules_are_judged_at_the_node_own_prefix() {
    let mut db = Db::new();
    let expect = |db: &mut Db, change: Change, finding: fn(EntryId) -> PlanningMessage| {
        let id = db.one(change);
        assert_eq!(judged(db, id), vec![finding(id)]);
        assert!(!judged_before(db, id).contains(&finding(id)));
    };
    let change = db.stance("pursued", &[]);
    expect(&mut db, change, |stance| UnknownKind {
        stance,
        kind: "pursued".into(),
    });
    let change = db.stance("committed", &[]);
    expect(&mut db, change, |stance| MissingCost { stance });
    let change = db.stance("deferred", &[]);
    expect(&mut db, change, |stance| MissingHorizon {
        stance,
        kind: StanceKind::Deferred,
    });
    let change = db.stance("waiting", &[]);
    expect(&mut db, change, |stance| MissingHorizon {
        stance,
        kind: StanceKind::Waiting,
    });
    let change = db.stance("committed", &[("cost", 1), ("horizon", 2)]);
    expect(&mut db, change, |stance| ForbiddenField {
        stance,
        kind: StanceKind::Committed,
        field: "horizon",
    });
    let change = db.stance("deferred", &[("cost", 1), ("horizon", 2)]);
    expect(&mut db, change, |stance| ForbiddenField {
        stance,
        kind: StanceKind::Deferred,
        field: "cost",
    });
    let change = db.stance("resumed", &[("horizon", 1)]);
    expect(&mut db, change, |stance| ForbiddenField {
        stance,
        kind: StanceKind::Resumed,
        field: "horizon",
    });
    let change = db.stance("committed", &[("cost", -1)]);
    expect(&mut db, change, |stance| NegativeCost { stance, cost: -1 });
    let change = db.stance("waiting", &[("horizon", 0)]);
    expect(&mut db, change, |stance| HorizonBelowOne {
        stance,
        horizon: 0,
    });
    // A negative budget is also overrun by the seal it is part of.
    let review = db.one(review_at(-1, at(DAY)));
    appears_at(&db, review, &NegativeBudget { review, budget: -1 });
    expect(&mut db, habit("Run", -1), |item| NegativeQuota {
        item,
        quota: -1,
    });
    expect(&mut db, concern("Parents", -2), |item| NegativeQuota {
        item,
        quota: -2,
    });
    // Every well-formed kind passes.
    for (kind, fields) in [
        ("committed", &[("cost", 0)][..]),
        ("deferred", &[("horizon", 1)]),
        ("waiting", &[("horizon", 30)]),
        ("resumed", &[]),
        ("sufficient", &[]),
        ("abandoned", &[]),
    ] {
        let change = db.stance(kind, fields);
        let stance = db.one(change);
        quiet(&db, stance);
    }
}

// Acceptance test 26, the predecessor half.
#[test]
fn reviews_and_events_are_never_revised() {
    let mut db = Db::new();
    let first = db.review(0);
    let again = db.one(node_change(
        "Review",
        Body::from([("budget".into(), 1.into()), ("at".into(), at(DAY).into())]),
        Some((Existing(first), RevisionIntent::Revise)),
    ));
    appears_at(
        &db,
        again,
        &RevisedBookkeeping {
            node: again,
            ty: PlanningType::Review,
        },
    );
    for ty in ["Done", "Did"] {
        let event = db.event(ty);
        let event = db.one(event);
        let mut payload = Body::new();
        payload.insert("at".into(), db.tick());
        let successor = db.one(node_change(
            ty,
            payload,
            Some((Existing(event), RevisionIntent::Revise)),
        ));
        let finding = RevisedBookkeeping {
            node: successor,
            ty: PlanningType::from_name(ty).unwrap(),
        };
        appears_at(&db, successor, &finding);
        assert_eq!(judged(&db, successor), vec![finding]);
    }
}

// Acceptance test 20.
#[test]
fn a_deferral_horizon_is_capped_at_the_stance_own_prefix() {
    let mut db = Db::new();
    let change = db.stance("deferred", &[("horizon", 12)]);
    let at_cap = db.one(change);
    quiet(&db, at_cap);
    let change = db.stance("deferred", &[("horizon", 13)]);
    let over = db.one(change);
    let finding = HorizonOverCap {
        stance: over,
        horizon: 13,
        cap: 12,
    };
    assert_eq!(judged(&db, over), vec![finding]);
    assert_eq!(judged_before(&db, over), vec![]);
    assert_eq!(
        judged_with(&db, over, r#"{"max_deferral_horizon": null}"#),
        vec![]
    );
    assert_eq!(
        judged_with(&db, over, r#"{"max_deferral_horizon": 13}"#),
        vec![]
    );
    assert_eq!(
        judged_with(&db, at_cap, r#"{"max_deferral_horizon": 3}"#),
        vec![HorizonOverCap {
            stance: at_cap,
            horizon: 12,
            cap: 3,
        }]
    );
    // A wait is not capped.
    let change = db.stance("waiting", &[("horizon", 100)]);
    let wait = db.one(change);
    quiet(&db, wait);
}

// Acceptance test 22.
#[test]
fn abandoned_and_sufficient_stances_are_final() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Fix the bike"));
    let goal = db.one(item("Goal", "Trip"));
    let (abandon, _) = db.decide("abandoned", &[], task);
    let (sufficient, _) = db.decide("sufficient", &[], goal);
    for (final_stance, kind) in [
        (abandon, StanceKind::Abandoned),
        (sufficient, StanceKind::Sufficient),
    ] {
        for (successor_kind, fields) in [
            ("resumed", &[][..]),
            ("deferred", &[("horizon", 1)]),
            ("abandoned", &[]),
        ] {
            let change = db.supersede(final_stance, successor_kind, fields);
            // A branch, since a rejected successor would have been undone.
            let Change::Entry(EntryChange::Node(mut node)) = change else {
                unreachable!()
            };
            node.intent = Some(RevisionIntent::Branch);
            let successor = db.one(Change::Entry(EntryChange::Node(node)));
            appears_at(
                &db,
                successor,
                &SuccessorOfFinalStance {
                    node: successor,
                    predecessor: final_stance,
                    kind,
                },
            );
        }
    }
    let other = db.one(item("Task", "Book lessons"));
    let (deferral, _) = db.defer(2, other);
    let change = db.supersede(deferral, "committed", &[("cost", 1)]);
    let commitment = db.one(change);
    quiet(&db, commitment);
    let change = db.supersede(commitment, "deferred", &[("horizon", 1)]);
    let again = db.one(change);
    quiet(&db, again);
}

// Acceptance tests 4 and 27: what may succeed what.
#[test]
fn terminal_revisions_are_final_and_items_never_succeed_bookkeeping() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Book lessons"));
    db.finish(task);
    let successor = db.one(revise("Task", "Book more lessons", task));
    appears_at(
        &db,
        successor,
        &SuccessorOfTerminal {
            node: successor,
            predecessor: task,
            state: ItemState::Done,
        },
    );
    let goal = db.one(item("Goal", "Trip"));
    db.decide("abandoned", &[], goal);
    let revived = db.one(revise("Goal", "Trip again", goal));
    assert_eq!(
        judged(&db, revived),
        vec![SuccessorOfTerminal {
            node: revived,
            predecessor: goal,
            state: ItemState::Abandoned,
        }]
    );

    // A stance whose predecessor is a task, and a task whose predecessor is
    // a `Did`.
    let open = db.one(item("Task", "Renew passport"));
    let mut payload = Body::from([("kind".into(), "resumed".into())]);
    payload.insert("at".into(), db.tick());
    let stance = db.one(node_change(
        "Stance",
        payload,
        Some((Existing(open), RevisionIntent::Revise)),
    ));
    appears_at(
        &db,
        stance,
        &CrossesBookkeeping {
            node: stance,
            from: PlanningType::Task,
            to: PlanningType::Stance,
        },
    );
    let did = db.event("Did");
    let did = db.one(did);
    let task_from_did = db.one(revise("Task", "From a Did", did));
    assert_eq!(
        judged(&db, task_from_did),
        vec![CrossesBookkeeping {
            node: task_from_did,
            from: PlanningType::Did,
            to: PlanningType::Task,
        }]
    );

    // Between item types, and into and out of the plan, is the user's.
    let promoted = db.one(revise("Project", "Passport and visa", stance_free(&db)));
    quiet(&db, promoted);
    let note = db.one(node_change(
        "Note",
        titled("Aside", None),
        Some((Existing(promoted), RevisionIntent::Revise)),
    ));
    quiet(&db, note);
    let back = db.one(revise("Task", "Back in the plan", note));
    quiet(&db, back);
}

// An open task with no stance, for transitions that must not be judged.
fn stance_free(db: &Db) -> EntryId {
    let current = crate::current::Current::new(db.view()).unwrap();
    current
        .items()
        .find(|&(id, ty)| {
            ty == PlanningType::Task && current.is_open(id) && current.current_stance(id).is_none()
        })
        .map(|(id, _)| id)
        .expect("an open task with no stance")
}

// ---------------------------------------------------------------------------
// Sections 8.2 and 8.3: at an edge's prefix

fn bad_endpoints(edge: EntryId, label: Label, from: &str, to: &str) -> PlanningMessage {
    BadEndpoints {
        edge,
        label,
        from_type: from.into(),
        to_type: to.into(),
    }
}

// Acceptance test 2.
#[test]
fn endpoint_pairs_are_those_of_section_3_2() {
    let mut db = Db::new();
    let of = |db: &mut Db, ty: &str, title: &str| db.one(item(ty, title));
    let goal = of(&mut db, "Goal", "goal");
    let concern = of(&mut db, "Concern", "concern");
    let project = of(&mut db, "Project", "project");
    let task = of(&mut db, "Task", "task");
    let other_task = of(&mut db, "Task", "other task");
    let habit = of(&mut db, "Habit", "habit");
    let external = of(&mut db, "External", "external");
    let note = of(&mut db, "Note", "note");
    let change = db.stance("deferred", &[("horizon", 1)]);
    let stance = db.one(change);
    let done = db.event("Done");
    let done = db.one(done);
    let did = db.event("Did");
    let did = db.one(did);
    let review = db.review(5);
    let base = db.graph.clone();
    for (label, from, to) in [
        ("Enables", task, goal),
        ("Enables", project, other_task),
        ("Enables", external, project),
        ("Helps", other_task, concern),
        ("Helps", habit, goal),
        ("Helps", external, task),
        ("Until", stance, external),
        ("About", stance, habit),
        ("About", done, task),
        ("About", did, concern),
    ] {
        let edge = db.link(label, from, to);
        quiet(&db, edge);
    }
    // Each rejected pair on a copy of the graph before any edge, so that one
    // judgment is not mixed with a cycle or a terminal target another made.
    for (label, from, to, from_type, to_type) in [
        (Label::Enables, goal, task, "Goal", "Task"),
        (Label::Enables, habit, concern, "Habit", "Concern"),
        (Label::Helps, concern, goal, "Concern", "Goal"),
        (Label::Enables, task, habit, "Task", "Habit"),
        (Label::Helps, task, note, "Task", "Note"),
        (Label::About, note, task, "Note", "Task"),
        (Label::About, task, goal, "Task", "Goal"),
        (Label::About, did, task, "Did", "Task"),
        (Label::About, done, goal, "Done", "Goal"),
        (Label::About, stance, review, "Stance", "Review"),
        (Label::Until, stance, goal, "Stance", "Goal"),
        (Label::Until, did, task, "Did", "Task"),
    ] {
        let mut db = Db {
            graph: base.clone(),
            clock: 0,
        };
        let edge = db.link(label.name(), from, to);
        let finding = bad_endpoints(edge, label, from_type, to_type);
        assert_eq!(judged(&db, edge), vec![finding.clone()]);
        assert!(!judged_before(&db, edge).contains(&finding));
    }
}

// Acceptance test 2: no edge into a terminal target.
#[test]
fn a_dependency_into_a_terminal_target_is_rejected_at_its_prefix() {
    let mut db = Db::new();
    let finished = db.one(item("Task", "Book lessons"));
    db.finish(finished);
    let goal = db.one(item("Goal", "Trip"));
    db.decide("abandoned", &[], goal);
    let task = db.one(item("Task", "Fix the bike"));
    let enables = db.link("Enables", task, finished);
    appears_at(
        &db,
        enables,
        &TerminalTarget {
            edge: enables,
            label: Label::Enables,
            state: ItemState::Done,
        },
    );
    let helps = db.link("Helps", task, goal);
    assert_eq!(
        judged(&db, helps),
        vec![TerminalTarget {
            edge: helps,
            label: Label::Helps,
            state: ItemState::Abandoned,
        }]
    );
    // A done source is fine: it is the prerequisite that is finished.
    let open = db.one(item("Task", "Attend lessons"));
    let from_done = db.link("Enables", finished, open);
    quiet(&db, from_done);
}

// Acceptance test 3.
#[test]
fn a_cycle_is_reported_once_per_component_at_the_closing_edge() {
    let mut db = Db::new();
    let [a, b, c] = db
        .apply(&[item("Task", "a"), item("Task", "b"), item("Task", "c")])
        .try_into()
        .unwrap();
    let ab = db.link("Enables", a, b);
    let bc = db.link("Helps", b, c);
    for edge in [ab, bc] {
        quiet(&db, edge);
    }
    // Mixed: Enables and Helps together.
    let ca = db.link("Enables", c, a);
    let component = lifecore_dag_check::CyclicComponent {
        nodes: vec![a, b, c],
        edges: vec![ab, bc, ca],
    };
    assert_eq!(
        judged(&db, ca),
        vec![Cycle {
            edge: ca,
            component: component.clone(),
        }]
    );
    assert_eq!(judged_before(&db, ca), vec![]);
    // A later edge that the existing cycle does not hold reports nothing.
    let d = db.one(item("Task", "d"));
    let ad = db.link("Enables", a, d);
    quiet(&db, ad);
    // Only Helps, and only Enables, each in a component of its own.
    let [x, y] = db
        .apply(&[item("Project", "x"), item("Task", "y")])
        .try_into()
        .unwrap();
    let xy = db.link("Helps", x, y);
    let yx = db.link("Helps", y, x);
    assert_eq!(
        judged(&db, yx),
        vec![Cycle {
            edge: yx,
            component: lifecore_dag_check::CyclicComponent {
                nodes: vec![x, y],
                edges: vec![xy, yx],
            },
        }]
    );
    let [p, q] = db
        .apply(&[item("Task", "p"), item("Task", "q")])
        .try_into()
        .unwrap();
    let pq = db.link("Enables", p, q);
    let qp = db.link("Enables", q, p);
    assert!(matches!(&judged(&db, qp)[..], [Cycle { edge, .. }] if *edge == qp));
    quiet(&db, pq);
    // An edge with another label closes no planning cycle.
    let other = db.link("Cites", b, a);
    quiet(&db, other);
}

// Acceptance tests 4 and 24.
#[test]
fn a_done_is_judged_against_its_target_prerequisites_at_its_about_prefix() {
    let mut db = Db::new();
    let book = db.one(item("Task", "Book lessons"));
    let attend = db.one(item("Task", "Attend lessons"));
    db.link("Enables", book, attend);
    let (early, early_about) = db.finish(attend);
    appears_at(
        &db,
        early_about,
        &PrematureCompletion {
            edge: early_about,
            completion: Completion::Done,
            sources: vec![book],
        },
    );
    quiet(&db, early);

    let mut db = Db::new();
    let book = db.one(item("Task", "Book lessons"));
    let attend = db.one(item("Task", "Attend lessons"));
    let enables = db.link("Enables", book, attend);
    let (_, booked) = db.finish(book);
    quiet(&db, booked);
    let (done, attended) = db.finish(attend);
    quiet(&db, attended);
    // The edge stays current: completion is an event, not a revision.
    assert_eq!(db.view().is_stale(enables), Some(false));
    let current = crate::current::Current::new(db.view()).unwrap();
    assert_eq!(current.state(book), ItemState::Done);
    assert_eq!(current.state(attend), ItemState::Done);

    // A second `Done` about the task makes it terminal twice over; its About
    // edge is the inconsistency, not its node.
    let (again, again_about) = db.finish(attend);
    quiet(&db, again);
    assert_eq!(
        judged(&db, again_about),
        vec![AboutTerminal {
            edge: again_about,
            state: ItemState::Done,
        }]
    );

    // A `Done` with no `About` edge completes nothing, and a second `About`
    // from one `Done` is rejected.
    let orphan = db.event("Done");
    let orphan = db.one(orphan);
    quiet(&db, orphan);
    let open = db.one(item("Task", "Renew passport"));
    assert_eq!(
        crate::current::Current::new(db.view()).unwrap().state(open),
        ItemState::Open
    );
    let second = db.link("About", done, open);
    appears_at(
        &db,
        second,
        &SecondDoneAbout {
            edge: second,
            earlier: attended,
        },
    );

    // An abandoned habit takes no `Did`.
    let run = db.one(habit("Run", 3));
    let (_, abandoned) = db.decide("abandoned", &[], run);
    quiet(&db, abandoned);
    let (_, did) = db.attend(run);
    appears_at(
        &db,
        did,
        &AboutTerminal {
            edge: did,
            state: ItemState::Abandoned,
        },
    );
}

// Acceptance test 26, the edge half, and test 28's abandonment rule.
#[test]
fn stance_kinds_apply_to_fixed_types_and_wait_for_prerequisites() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Trip"));
    let task = db.one(item("Task", "Passport"));
    db.link("Enables", task, goal);
    let (_, committed_goal) = db.commit(1, goal);
    appears_at(
        &db,
        committed_goal,
        &KindDoesNotApply {
            edge: committed_goal,
            kind: StanceKind::Committed,
            target_type: PlanningType::Goal,
        },
    );
    let (_, waiting_task) = db.decide("waiting", &[("horizon", 1)], task);
    assert_eq!(
        judged(&db, waiting_task),
        vec![KindDoesNotApply {
            edge: waiting_task,
            kind: StanceKind::Waiting,
            target_type: PlanningType::Task,
        }]
    );
    let other = db.one(item("Task", "Other"));
    let (_, sufficient_task) = db.decide("sufficient", &[], other);
    assert_eq!(
        judged(&db, sufficient_task),
        vec![KindDoesNotApply {
            edge: sufficient_task,
            kind: StanceKind::Sufficient,
            target_type: PlanningType::Task,
        }]
    );

    // `Until` only from a deferral.
    let change = db.stance("committed", &[("cost", 1)]);
    let commitment = db.one(change);
    let until = db.link("Until", commitment, task);
    appears_at(
        &db,
        until,
        &UntilFromNonDeferral {
            edge: until,
            kind: StanceKind::Committed,
        },
    );

    // `sufficient` waits for every `Enables` source to be done, `abandoned`
    // for none of them to be open.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Trip"));
    let passport = db.one(item("Task", "Passport"));
    let bike = db.one(item("Task", "Bike"));
    db.link("Enables", passport, goal);
    db.link("Enables", bike, goal);
    // The findings at the `About` edge of a `kind` stance about `goal`,
    // appended to a copy of `db`.
    let check = |db: &Db, kind: &str, goal: EntryId| {
        let mut alt = Db {
            graph: db.graph.clone(),
            clock: db.clock,
        };
        let (_, about) = alt.decide(kind, &[], goal);
        judged(&alt, about)
    };
    let sufficient = check(&db, "sufficient", goal);
    let abandoned = check(&db, "abandoned", goal);
    assert!(matches!(
        &sufficient[..],
        [PrematureCompletion { completion: Completion::Sufficient, sources, .. }] if *sources == vec![passport, bike]
    ));
    assert!(matches!(
        &abandoned[..],
        [PrematureCompletion { completion: Completion::Abandoned, sources, .. }] if *sources == vec![passport, bike]
    ));
    // Abandoning one prerequisite and finishing the other: `abandoned` passes,
    // `sufficient` still waits for the abandoned one.
    db.decide("abandoned", &[], bike);
    db.finish(passport);
    assert!(check(&db, "abandoned", goal).is_empty());
    assert!(matches!(
        &check(&db, "sufficient", goal)[..],
        [PrematureCompletion { completion: Completion::Sufficient, sources, .. }] if *sources == vec![bike]
    ));
    // A prerequisite revised without its edge no longer waits.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Trip"));
    let passport = db.one(item("Task", "Passport"));
    db.link("Enables", passport, goal);
    assert!(!check(&db, "abandoned", goal).is_empty());
    db.one(revise("Task", "Passport, if ever", passport));
    assert!(check(&db, "abandoned", goal).is_empty());
}

// Acceptance tests 7 and 21: a revision has at most one current stance.
#[test]
fn a_second_current_stance_is_rejected_and_supersession_passes() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Fix the bike"));
    let (deferral, first) = db.defer(2, task);
    quiet(&db, first);
    let (_, second) = db.commit(1, task);
    appears_at(
        &db,
        second,
        &SecondCurrentStance {
            edge: second,
            current: deferral,
        },
    );
    // A root `resumed` about a deferred item is a second stance too.
    let run = db.one(habit("Run", 3));
    let (deferral, _) = db.defer(4, run);
    let (_, root_resume) = db.decide("resumed", &[], run);
    assert_eq!(
        judged(&db, root_resume),
        vec![SecondCurrentStance {
            edge: root_resume,
            current: deferral,
        }]
    );
    // Superseding passes: the old stance's edges are historical by the time
    // the new one's is appended.
    let mut db = Db::new();
    let task = db.one(item("Task", "Fix the bike"));
    let (deferral, _) = db.defer(2, task);
    let (_, superseding) = db.redecide(deferral, "committed", &[("cost", 1)], task);
    quiet(&db, superseding);
    let run = db.one(habit("Run", 3));
    let (deferral, _) = db.defer(4, run);
    let (resumed, resumed_about) = db.redecide(deferral, "resumed", &[], run);
    quiet(&db, resumed);
    quiet(&db, resumed_about);
}

// Acceptance test 23.
#[test]
fn a_stance_or_did_is_re_pointed_only_to_a_descendant_of_an_earlier_target() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Attend lessons"));
    let (stance, _) = db.commit(2, task);
    let revised = db.one(revise("Task", "Attend ten lessons", task));
    let repointed = db.link("About", stance, revised);
    quiet(&db, repointed);
    // Both branches of a split descend from the earlier targets.
    let club = db.one(revise("Task", "Club lessons", revised));
    let private = db.one(branch("Task", "Private lessons", revised));
    let to_club = db.link("About", stance, club);
    let to_private = db.link("About", stance, private);
    quiet(&db, to_club);
    quiet(&db, to_private);
    // An unrelated task does not.
    let unrelated = db.one(item("Task", "Fix the bike"));
    let stray = db.link("About", stance, unrelated);
    appears_at(
        &db,
        stray,
        &UnrelatedRepoint {
            edge: stray,
            label: Label::About,
        },
    );
    // Nor does a second `About` edge, in one plan, to an unrelated item.
    let one = db.one(item("Task", "Renew passport"));
    let other = db.one(item("Task", "Book flights"));
    let change = db.stance("deferred", &[("horizon", 2)]);
    let [_, first, second] = db
        .apply(&[
            change,
            edge("About", Added(0), Existing(one)),
            edge("About", Added(0), Existing(other)),
        ])
        .try_into()
        .unwrap();
    quiet(&db, first);
    appears_at(
        &db,
        second,
        &UnrelatedRepoint {
            edge: second,
            label: Label::About,
        },
    );
    // The same holds for a `Did`.
    let run = db.one(habit("Run", 3));
    let (did, _) = db.attend(run);
    let jog = db.one(revise("Habit", "Jog", run));
    let followed = db.link("About", did, jog);
    quiet(&db, followed);
    let swim = db.one(habit("Swim", 1));
    let wrong = db.link("About", did, swim);
    assert_eq!(
        judged(&db, wrong),
        vec![UnrelatedRepoint {
            edge: wrong,
            label: Label::About,
        }]
    );
}

// Acceptance test 9, the edge half.
#[test]
fn an_until_edge_is_re_pointed_like_about_and_has_one_current_target() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Live abroad"));
    let certificate = db.one(item("Task", "Language certificate"));
    let move_out = db.one(item("Project", "Move"));
    let (deferral, _) = db.defer(3, certificate);
    let until = db.link("Until", deferral, move_out);
    quiet(&db, until);
    // An edge between types §3.2 does not permit has nothing more to judge.
    let second = db.link("Until", deferral, goal);
    assert_eq!(
        judged(&db, second),
        vec![bad_endpoints(second, Label::Until, "Stance", "Goal")]
    );
    let mut db2 = Db::new();
    let certificate = db2.one(item("Task", "Language certificate"));
    let move_out = db2.one(item("Project", "Move"));
    let elsewhere = db2.one(item("Project", "Elsewhere"));
    let (deferral, _) = db2.defer(3, certificate);
    db2.link("Until", deferral, move_out);
    let moved = db2.one(revise("Project", "Move in May", move_out));
    let repointed = db2.link("Until", deferral, moved);
    quiet(&db2, repointed);
    let stray = db2.link("Until", deferral, elsewhere);
    assert_eq!(
        judged(&db2, stray),
        vec![
            SecondUntil {
                edge: stray,
                current: repointed,
            },
            UnrelatedRepoint {
                edge: stray,
                label: Label::Until,
            },
        ]
    );
}

// Acceptance test 17.
#[test]
fn entries_outside_the_vocabulary_are_not_judged() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Fix the bike"));
    let goal = db.one(item("Goal", "Trip"));
    let note = db.one(item("Note", "Manual"));
    let note_type = db.one(declare(&Type::new(
        "Note",
        &[],
        &["Cites"],
        &["Cites"],
        &[],
    )));
    let own_label = db.link("Cites", task, goal);
    let note_edge = db.link("Cites", note, task);
    let revised_note = db.one(revise("Note", "Manual, second edition", note));
    for id in [note, note_type, own_label, note_edge, revised_note] {
        quiet(&db, id);
    }
    // Planning owns `About`, whatever the user meant by it (decision 35).
    let about = db.link("About", revised_note, task);
    appears_at(
        &db,
        about,
        &bad_endpoints(about, Label::About, "Note", "Task"),
    );
}

// ---------------------------------------------------------------------------
// Sections 6, 7, 8.4, and 8.5: lifetimes and seals

fn undecided(revision: EntryId, review: EntryId) -> PlanningMessage {
    Undecided { revision, review }
}

// Acceptance test 7.
#[test]
fn a_seal_asks_each_ready_task_in_play_for_a_decision() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let book = db.one(item("Task", "Book lessons"));
    let attend = db.one(item("Task", "Attend lessons"));
    db.link("Enables", book, attend);
    db.link("Enables", attend, goal);

    let mut bare = db.fork();
    let review = bare.review(3);
    // Only the ready task: the blocked one needs no decision, and the goal
    // is neither ready nor broken.
    assert_eq!(judged(&bare, review), vec![undecided(book, review)]);
    assert_eq!(judged_before(&bare, review), vec![]);

    let mut committed = db.fork();
    committed.commit(1, book);
    let review = committed.review(3);
    quiet(&committed, review);

    let mut deferred = db.fork();
    deferred.defer(1, book);
    let review = deferred.review(3);
    quiet(&deferred, review);
    let seal = sealed_at(&deferred, review);
    assert!(seal.is_dormant(book));
    // Dormant through what it serves: `attend` serves the goal, in play.
    assert!(seal.in_play(attend));

    // A successor stance replaces its predecessor for the budget and the
    // decision.
    let mut replaced = db.fork();
    let (expensive, _) = replaced.commit(5, book);
    replaced.redecide(expensive, "committed", &[("cost", 1)], book);
    let review = replaced.review(1);
    quiet(&replaced, review);
    let mut deferred_instead = db.fork();
    let (expensive, _) = deferred_instead.commit(5, book);
    deferred_instead.redecide(expensive, "deferred", &[("horizon", 1)], book);
    let review = deferred_instead.review(0);
    quiet(&deferred_instead, review);
}

// Acceptance test 8.
#[test]
fn the_budget_counts_each_fresh_commitment_once() {
    let mut db = Db::new();
    let a = db.one(item("Task", "a"));
    let b = db.one(item("Task", "b"));
    let (sa, _) = db.commit(2, a);
    let (sb, _) = db.commit(3, b);
    let mut equal = db.fork();
    let review = equal.review(5);
    quiet(&equal, review);
    let review = db.review(4);
    assert_eq!(
        judged(&db, review),
        vec![BudgetOverrun {
            review,
            stances: vec![sa, sb],
            cost: 5,
            budget: 4,
        }]
    );

    // A commitment under a deferred goal keeps its task in play and counts.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Trip"));
    let task = db.one(item("Task", "Passport"));
    db.link("Enables", task, goal);
    db.defer(3, goal);
    let (stance, _) = db.commit(3, task);
    let review = db.review(2);
    assert_eq!(
        judged(&db, review),
        vec![BudgetOverrun {
            review,
            stances: vec![stance],
            cost: 3,
            budget: 2,
        }]
    );
    assert!(sealed_at(&db, review).in_play(task));

    // A stance re-pointed to both halves of a split counts once.
    let mut db = Db::new();
    let task = db.one(item("Task", "Lessons"));
    let (stance, _) = db.commit(3, task);
    let club = db.one(revise("Task", "Club", task));
    let private = db.one(branch("Task", "Private", task));
    db.link("About", stance, club);
    db.link("About", stance, private);
    let mut enough = db.fork();
    let review = enough.review(3);
    quiet(&enough, review);
    let review = db.review(2);
    assert_eq!(
        judged(&db, review),
        vec![BudgetOverrun {
            review,
            stances: vec![stance],
            cost: 3,
            budget: 2,
        }]
    );

    // A standing commitment is the last window's plan, not this one's.
    let mut db = Db::new();
    let task = db.one(item("Task", "Lessons"));
    db.commit(3, task);
    db.review(3);
    let review = db.review(0);
    assert_eq!(judged(&db, review), vec![undecided(task, review)]);
}

// Acceptance test 9: committed and deferred lifetimes.
#[test]
fn a_commitment_lapses_at_the_next_seal_and_a_deferral_after_its_horizon() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Lessons"));
    db.commit(1, task);
    let first = db.review(3);
    quiet(&db, first);
    assert_eq!(open_window(&db).decision(task), Some(StanceKind::Committed));
    let second = db.review(3);
    assert_eq!(judged(&db, second), vec![undecided(task, second)]);

    // Sealed by Review 1 with horizon 2, it stands through Review 3 and
    // lapses after it; the task returns with no new entry.
    let mut db = Db::new();
    let task = db.one(item("Task", "Passport"));
    let (_, about) = db.defer(2, task);
    assert!(in_open_window_at(&db, about).is_dormant(task));
    for _ in 0..3 {
        let review = db.review(3);
        quiet(&db, review);
        assert!(sealed_at(&db, review).is_dormant(task));
    }
    assert!(open_window(&db).in_play(task));
    let fourth = db.review(3);
    assert_eq!(judged(&db, fourth), vec![undecided(task, fourth)]);
}

// Acceptance test 9: a deferral `Until` another item.
#[test]
fn a_deferral_lapses_early_when_its_until_target_is_terminal_or_left_behind() {
    // Done, or abandoned: the deferral ends at that boundary.
    for finish in ["done", "abandoned"] {
        let mut db = Db::new();
        let task = db.one(item("Task", "Certificate"));
        let waits_on = db.one(item("External", "Visa"));
        let (wait, _) = db.decide("waiting", &[("horizon", 9)], waits_on);
        let (deferral, _) = db.defer(5, task);
        db.link("Until", deferral, waits_on);
        let first = db.review(3);
        assert!(judged(&db, first).is_empty());
        assert!(open_window(&db).is_dormant(task));
        let (_, ended) = match finish {
        "done" => db.finish(waits_on),
        _ => db.redecide(wait, "abandoned", &[], waits_on),
        };
        quiet(&db, ended);
        // The event's node alone completes nothing; its `About` edge does.
        let event = db.view().at(ended.position() - 1).unwrap().id();
        assert!(in_open_window_at(&db, event).is_dormant(task));
        assert!(in_open_window_at(&db, ended).in_play(task));
        let second = db.review(3);
        assert_eq!(judged(&db, second), vec![undecided(task, second)]);
    }

    // Re-pointed before the seal, the deferral keeps its clock: sealed by
    // Review 1 with horizon 3, it stands through Review 4.
    let mut db = Db::new();
    let task = db.one(item("Task", "Certificate"));
    let waits_on = db.one(item("Project", "Move"));
    let (deferral, _) = db.defer(3, task);
    db.link("Until", deferral, waits_on);
    db.review(3);
    let mut repointed = db.fork();
    let moved = repointed.one(revise("Project", "Move in May", waits_on));
    // Between the revision and the next seal the deferral still stands.
    assert!(open_window(&repointed).is_dormant(task));
    repointed.link("Until", deferral, moved);
    for _ in 2..=4 {
        let review = repointed.review(3);
        assert!(sealed_at(&repointed, review).is_dormant(task));
    }
    let fifth = repointed.review(3);
    assert!(judged(&repointed, fifth).contains(&undecided(task, fifth)));

    // Left unanswered, it stands through that seal and lapses after it, and
    // re-pointing later does not bring it back.
    let mut unanswered = db.fork();
    let moved = unanswered.one(revise("Project", "Move in May", waits_on));
    let second = unanswered.review(3);
    assert!(sealed_at(&unanswered, second).is_dormant(task));
    assert!(open_window(&unanswered).in_play(task));
    unanswered.link("Until", deferral, moved);
    assert!(open_window(&unanswered).in_play(task));
    let third = unanswered.review(3);
    assert!(judged(&unanswered, third).contains(&undecided(task, third)));
}

// Acceptance test 5.
#[test]
fn a_split_task_yields_two_frontier_revisions_with_one_age() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let task = db.one(item("Task", "Lessons"));
    db.link("Enables", task, goal);
    let (first, _) = db.commit(1, task);
    db.review(3);
    // The next window's commitment, made before the split.
    let (fresh, _) = db.redecide(first, "committed", &[("cost", 2)], task);
    let club = db.one(revise("Task", "Club", task));
    let private = db.one(branch("Task", "Private", task));
    db.link("Enables", club, goal);
    db.link("Enables", private, goal);
    let mut bare = db.fork();
    let review = bare.review(3);
    let findings = judged(&bare, review);
    assert!(findings.contains(&undecided(club, review)));
    assert!(findings.contains(&undecided(private, review)));

    // Re-pointed to both halves, the one fresh commitment decides both, and
    // its cost counts once.
    db.link("About", fresh, club);
    db.link("About", fresh, private);
    let review = db.review(2);
    quiet(&db, review);
    // Ordered by their own positions, sharing the age of their root.
    let tier1 = ranking(&db)
        .into_iter()
        .filter(|(_, tier, _)| *tier == 1)
        .collect::<Vec<_>>();
    assert_eq!(
        tier1,
        vec![
            (club, 1, vec![Reason::OpenFor { seals: 2 }]),
            (private, 1, vec![Reason::OpenFor { seals: 2 }]),
        ]
    );
    db.finish(club);
    let current = crate::current::Current::new(db.view()).unwrap();
    assert_eq!(current.state(club), ItemState::Done);
    assert_eq!(current.state(private), ItemState::Open);
}

// Acceptance test 6.
#[test]
fn a_revision_drops_its_connections_until_they_are_re_pointed() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let task = db.one(item("Task", "Attend 8 lessons"));
    let enables = db.link("Enables", task, goal);
    let (stance, about) = db.commit(2, task);
    let revised = db.one(revise("Task", "Attend 10 lessons", task));

    let mut dropped = db.fork();
    let review = dropped.review(3);
    let findings = judged(&dropped, review);
    assert!(findings.contains(&undecided(revised, review)));
    assert!(findings.contains(&BrokenPlan { item: goal, review }));
    for (edge, label) in [(enables, Label::Enables), (about, Label::About)] {
        assert!(findings.contains(&DroppedConnection {
            edge,
            label,
            superseded: vec![Superseded {
                revision: task,
                successors: vec![revised],
            }],
            review,
        }));
    }
    assert_eq!(findings.len(), 4);

    // Re-pointed, the same stance decides the revision with its own
    // lifetime: it is still fresh, so it still counts toward the budget.
    db.link("Enables", revised, goal);
    db.link("About", stance, revised);
    let review = db.review(1);
    assert_eq!(
        judged(&db, review),
        vec![BudgetOverrun {
            review,
            stances: vec![stance],
            cost: 2,
            budget: 1,
        }]
    );
}

// Acceptance test 29.
#[test]
fn a_dropped_connection_is_reported_once() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let task = db.one(item("Task", "Lessons"));
    let enables = db.link("Enables", task, goal);
    db.commit(1, task);
    db.review(3);
    let revised = db.one(revise("Task", "More lessons", task));
    db.link("Enables", revised, goal);
    let first = db.review(3);
    assert!(
        judged(&db, first)
            .iter()
            .any(|f| matches!(f, DroppedConnection { .. }))
    );
    // The `About` edge was dropped; the Enables edge was re-pointed.
    assert!(
        !judged(&db, first)
            .iter()
            .any(|f| matches!(f, DroppedConnection { edge, .. } if *edge == enables))
    );
    let second = db.review(3);
    assert!(
        !judged(&db, second)
            .iter()
            .any(|f| matches!(f, DroppedConnection { .. }))
    );
    // A stance superseded with its `About` edge carried over drops nothing.
    let (stance, _) = db.commit(1, revised);
    db.redecide(stance, "committed", &[("cost", 2)], revised);
    let third = db.review(3);
    assert!(
        !judged(&db, third)
            .iter()
            .any(|f| matches!(f, DroppedConnection { .. }))
    );
}

// Acceptance test 10.
#[test]
fn work_that_only_helps_is_owed_a_decision() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Health"));
    let task = db.one(item("Task", "Buy running shoes"));
    db.link("Helps", task, goal);
    let mut bare = db.fork();
    let review = bare.review(3);
    // The goal has no `Enables` source and no habit, so it is broken too.
    assert_eq!(
        judged(&bare, review),
        vec![undecided(task, review), BrokenPlan { item: goal, review },]
    );
    assert_eq!(row(&db, task), Some((0, vec![Reason::Undecided])));

    let mut deferred = db.fork();
    deferred.defer(2, goal);
    let review = deferred.review(3);
    quiet(&deferred, review);
    assert!(sealed_at(&deferred, review).is_dormant(task));

    // Once what it helped is terminal, it serves nothing and stands alone.
    let mut abandoned = db.fork();
    abandoned.decide("abandoned", &[], goal);
    let review = abandoned.review(3);
    assert_eq!(judged(&abandoned, review), vec![undecided(task, review)]);

    let mut db = Db::new();
    let project = db.one(item("Project", "Move"));
    let task = db.one(item("Task", "Language course"));
    db.link("Helps", task, project);
    db.defer(1, task);
    db.finish(project);
    let review = db.review(3);
    quiet(&db, review);
    let review = db.review(3);
    quiet(&db, review);
    // The deferral has lapsed; nothing it served is left to keep it dormant.
    let review = db.review(3);
    assert_eq!(judged(&db, review), vec![undecided(task, review)]);
}

// Acceptance test 11.
#[test]
fn habits_are_decided_by_doing_after_a_grace_period() {
    let mut db = Db::new();
    let run = db.one(habit("Run", 2));
    let first = db.review(3);
    assert_eq!(
        judged(&db, first),
        vec![GracePeriod {
            item: run,
            review: first,
        }]
    );
    // Revising it does not restart the grace.
    let jog = db.one(node_change(
        "Habit",
        titled("Jog", Some(2)),
        Some((Existing(run), RevisionIntent::Revise)),
    ));
    let mut bare = db.fork();
    let second = bare.review(3);
    assert_eq!(
        judged(&bare, second),
        vec![
            undecided(jog, second),
            Shortfall {
                item: jog,
                review: second,
                dids: vec![],
                quota: 2,
            },
        ]
    );
    // A single `Did` decides it, below quota.
    let (did, _) = db.attend(jog);
    let second = db.review(3);
    assert_eq!(
        judged(&db, second),
        vec![Shortfall {
            item: jog,
            review: second,
            dids: vec![did],
            quota: 2,
        }]
    );
    // Deferred, it is neither asked about nor short.
    db.defer(1, jog);
    for _ in 0..2 {
        let review = db.review(3);
        quiet(&db, review);
    }
    // Lapsed, it is judged again.
    let review = db.review(3);
    assert_eq!(judged(&db, review).len(), 2);
    assert!(judged(&db, review).contains(&undecided(jog, review)));

    // Through the runner, the seal warnings are sticky.
    let mut db = Db::planned("");
    let run = db.one(habit("Run", 2));
    db.review(3);
    let (_, _) = db.attend(run);
    db.review(3);
    let output = runner().run(&db.graph).unwrap();
    let sticky = output
        .warnings()
        .map(|o| (o.diagnostic.sticky, o.diagnostic.focus))
        .collect::<Vec<_>>();
    assert_eq!(sticky, vec![(true, Some(run)), (true, Some(run))]);
}

// Acceptance test 12.
#[test]
fn a_plan_is_broken_when_nothing_can_advance_it() {
    // Nothing ready and nothing waited on.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Sabbatical"));
    let flights = db.one(item("Task", "Book flights"));
    let visa = db.one(item("External", "Visa"));
    db.link("Enables", visa, flights);
    db.link("Enables", flights, goal);
    let mut bare = db.fork();
    let review = bare.review(3);
    assert_eq!(
        judged(&bare, review),
        vec![undecided(visa, review), BrokenPlan { item: goal, review },]
    );
    // A `waiting` external rescues it, and decides itself.
    let mut waiting = db.fork();
    waiting.decide("waiting", &[("horizon", 2)], visa);
    let review = waiting.review(3);
    quiet(&waiting, review);
    // A deferred goal is not judged.
    let mut deferred = db.fork();
    deferred.defer(2, goal);
    let review = deferred.review(3);
    quiet(&deferred, review);

    // A goal with no prerequisite is broken unless a habit in play helps it;
    // helping work does not rescue it.
    let mut db = Db::new();
    let alone = db.one(item("Goal", "Cook"));
    let helped = db.one(item("Goal", "Fitness"));
    let book = db.one(item("Task", "Buy a cookbook"));
    db.link("Helps", book, alone);
    let run = db.one(habit("Run", 3));
    db.link("Helps", run, helped);
    let review = db.review(3);
    assert_eq!(
        judged(&db, review),
        vec![
            undecided(book, review),
            BrokenPlan {
                item: alone,
                review
            },
            GracePeriod { item: run, review },
        ]
    );

    // A ready project is undecided until its `Done`, which leaves nothing to
    // judge; then its goal is ready and undecided until `sufficient`.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Live abroad"));
    let project = db.one(item("Project", "Sabbatical"));
    db.link("Enables", project, goal);
    let review = db.review(3);
    assert_eq!(judged(&db, review), vec![undecided(project, review)]);
    let (_, done) = db.finish(project);
    quiet(&db, done);
    let review = db.review(3);
    assert_eq!(judged(&db, review), vec![undecided(goal, review)]);
    assert_eq!(row(&db, goal), Some((0, vec![Reason::Undecided])));
    let mut sufficient = db.fork();
    let (_, about) = sufficient.decide("sufficient", &[], goal);
    quiet(&sufficient, about);
    let review = sufficient.review(3);
    quiet(&sufficient, review);
    let mut later = db.fork();
    later.defer(1, goal);
    let review = later.review(3);
    quiet(&later, review);
}

// Acceptance test 19.
#[test]
fn deferring_a_goal_makes_dormant_what_serves_only_it() {
    let mut db = Db::new();
    let trip = db.one(item("Goal", "Trip"));
    let other = db.one(item("Goal", "Fitness"));
    let passport = db.one(item("Task", "Passport"));
    let bike = db.one(item("Task", "Bike"));
    let shared = db.one(item("Task", "Training plan"));
    let map = db.one(item("Task", "Buy a map"));
    let route = db.one(item("Task", "Plan the route"));
    let run = db.one(habit("Run", 1));
    db.link("Enables", passport, trip);
    db.link("Enables", bike, trip);
    db.link("Enables", shared, trip);
    db.link("Helps", shared, other);
    db.link("Enables", map, route);
    db.link("Enables", route, trip);
    db.link("Helps", run, trip);
    db.review(3);
    let (_, about) = db.defer(1, trip);
    let (commitment, _) = db.commit(2, route);
    let quieted = [passport, bike, run];
    // A commitment under the deferred goal keeps it and its prerequisites in
    // play; a task that also serves a goal in play stays in play.
    let review = db.review(5);
    // Dormant at every boundary from the deferral's `About` edge through its
    // sealing review; the committed task only until its commitment.
    for position in about.position()..review.position() {
        let situation = in_open_window_at(&db, id_at(&db, position));
        for id in quieted {
            assert!(situation.is_dormant(id), "{id:?} at {position}");
        }
        assert_eq!(
            situation.is_dormant(route),
            position < commitment.position() + 1
        );
    }
    let seal = sealed_at(&db, review);
    for id in quieted {
        assert!(seal.is_dormant(id));
    }
    assert!(seal.is_dormant(trip));
    for id in [route, map, shared, other] {
        assert!(seal.in_play(id), "{id:?}");
    }
    let findings = judged(&db, review);
    assert!(!findings.contains(&BrokenPlan { item: trip, review }));
    assert!(findings.contains(&undecided(map, review)));
    assert!(findings.contains(&undecided(shared, review)));
    // The deferral stands through the next seal and lapses after it.
    let last = db.review(5);
    assert!(sealed_at(&db, last).is_dormant(passport));
    let after = db.review(5);
    let seal = sealed_at(&db, after);
    for id in [passport, bike, run, trip] {
        assert!(seal.in_play(id));
    }
    assert!(judged(&db, after).contains(&undecided(passport, after)));

    // A habit that helps only an abandoned goal serves nothing.
    let mut db = Db::new();
    let goal = db.one(item("Goal", "10 km"));
    let run = db.one(habit("Run", 1));
    db.link("Helps", run, goal);
    db.decide("abandoned", &[], goal);
    assert!(open_window(&db).in_play(run));
}

// Acceptance test 21.
#[test]
fn a_resumed_stance_ends_a_deferral_and_decides_nothing() {
    let mut db = Db::new();
    let run = db.one(habit("Run", 1));
    db.review(3);
    let (deferral, _) = db.defer(4, run);
    let review = db.review(3);
    quiet(&db, review);
    assert!(in_open_window_at(&db, review).is_dormant(run));
    // Superseding the deferral ends it at once, since its `About` edge is
    // historical from the successor's own prefix.
    let (resumed, about) = db.redecide(deferral, "resumed", &[], run);
    assert!(in_open_window_at(&db, resumed).in_play(run));
    assert!(in_open_window_at(&db, about).in_play(run));
    let review = db.review(3);
    assert_eq!(
        judged(&db, review),
        vec![
            undecided(run, review),
            Shortfall {
                item: run,
                review,
                dids: vec![],
                quota: 1,
            },
        ]
    );

    let mut db = Db::new();
    let task = db.one(item("Task", "Passport"));
    let (deferral, _) = db.defer(4, task);
    db.redecide(deferral, "resumed", &[], task);
    let review = db.review(3);
    assert_eq!(judged(&db, review), vec![undecided(task, review)]);
    // About an item that was not deferred, it changes nothing.
    let other = db.one(item("Task", "Bike"));
    db.decide("resumed", &[], other);
    let review = db.review(3);
    assert_eq!(
        judged(&db, review),
        vec![undecided(task, review), undecided(other, review),]
    );
}

// Acceptance test 28.
#[test]
fn an_abandoned_prerequisite_blocks_its_target_until_it_is_revised_or_abandoned() {
    let mut db = Db::new();
    let project = db.one(item("Project", "Sabbatical"));
    let task = db.one(item("Task", "Sublet"));
    let enables = db.link("Enables", task, project);
    db.decide("abandoned", &[], task);
    let mut blocked = db.fork();
    let review = blocked.review(3);
    let finding = BlockedByAbandoned {
        edge: enables,
        source: task,
        target: project,
        review,
    };
    assert_eq!(
        judged(&blocked, review),
        vec![
            BrokenPlan {
                item: project,
                review
            },
            finding
        ]
    );
    // Revised without the edge, the project is ready.
    let mut revised = db.fork();
    let without = revised.one(revise("Project", "Sabbatical, no sublet", project));
    let review = revised.review(3);
    assert_eq!(
        judged(&revised, review),
        vec![
            undecided(without, review),
            DroppedConnection {
                edge: enables,
                label: Label::Enables,
                superseded: vec![Superseded {
                    revision: project,
                    successors: vec![without],
                }],
                review,
            },
        ]
    );
    // Abandoned too, nothing is left to judge.
    let mut abandoned = db.fork();
    let (_, about) = abandoned.decide("abandoned", &[], project);
    quiet(&abandoned, about);
    let review = abandoned.review(3);
    quiet(&abandoned, review);
    // Dormant, it is not judged.
    let mut dormant = db.fork();
    dormant.defer(1, project);
    let review = dormant.review(3);
    quiet(&dormant, review);
}

// ---------------------------------------------------------------------------
// Section 9: the valuation

fn dated(ty: &str, title: &str, deadline: Timestamp) -> Change {
    let mut payload = titled(title, None);
    payload.insert("deadline".into(), deadline.into());
    node_change(ty, payload, None)
}

// Acceptance test 13: before the first review.
#[test]
fn before_the_first_review_tiers_one_to_four_are_empty() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let task = db.one(item("Task", "Book lessons"));
    db.link("Enables", task, goal);
    db.commit(1, task);
    let run = db.one(habit("Run", 3));
    db.link("Helps", run, goal);
    let other = db.one(item("Task", "Buy goggles"));
    db.link("Enables", other, goal);
    let deferred = db.one(item("Task", "Swim club"));
    db.link("Enables", deferred, goal);
    db.defer(2, deferred);
    // A fresh commitment is the plan for the next window, a fresh deferral
    // makes its task dormant, and every habit is new.
    let rows = ranking(&db);
    assert_eq!(
        rows,
        vec![
            (other, 0, vec![Reason::Undecided]),
            (goal, 5, vec![]),
            (task, 5, vec![]),
            (run, 5, vec![Reason::New]),
        ]
    );
    assert_eq!(rows, ranking(&db));
}

// Acceptance test 13: every tier, with its keys and reasons.
#[test]
fn the_ranking_orders_tiers_by_their_keys() {
    let mut db = Db::new();
    // Window 0.
    let swim = db.one(item("Goal", "Swim"));
    let book = db.one(item("Task", "Book lessons"));
    let pool = db.one(item("External", "Pool reopens"));
    let gear = db.one(item("Task", "Buy gear"));
    for source in [book, pool, gear] {
        db.link("Enables", source, swim);
    }
    db.commit(1, book);
    db.decide("waiting", &[("horizon", 2)], pool);
    db.defer(3, gear);
    let cook = db.one(item("Goal", "Cook"));
    let cookbook = db.one(item("Task", "Cookbook"));
    let knives = db.one(item("Task", "Knives"));
    db.link("Enables", cookbook, cook);
    db.link("Enables", knives, cook);
    db.commit(1, cookbook);
    db.defer(5, knives);
    let read = db.one(item("Goal", "Read more"));
    let card = db.one(item("Task", "Library card"));
    db.link("Enables", card, read);
    db.defer(5, card);
    let run = db.one(habit("Run", 2));
    db.link("Helps", run, swim);
    let parents = db.one(concern("Parents", 1));
    let first = db.review(3);
    // Window 1.
    db.finish(book);
    db.finish(cookbook);
    db.attend(run);
    db.attend(parents);
    let recipe = db.one(item("Task", "One recipe"));
    db.link("Enables", recipe, cook);
    db.commit(1, recipe);
    let trip = db.one(item("Goal", "Trip"));
    let passport = db.one(item("Task", "Passport"));
    db.link("Enables", passport, trip);
    db.defer(1, trip);
    let second = db.review(3);
    for review in [first, second] {
        assert!(
            !judged(&db, review).iter().any(|f| f.ty_is_inconsistency()),
            "{:?}",
            judged(&db, review)
        );
    }
    // Window 2.
    db.finish(recipe);
    db.attend(run);
    let swim_habit = db.one(habit("Swim twice", 2));
    assert_eq!(
        ranking(&db),
        vec![
            (parents, 0, vec![Reason::Undecided]),
            (run, 2, vec![Reason::Shortfall { have: 1, quota: 2 }]),
            (
                pool,
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Waiting,
                        seals_left: 1
                    },
                    Reason::LapsesAtNextSeal,
                    Reason::OpenFor { seals: 2 },
                ]
            ),
            (
                trip,
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 1
                    },
                    Reason::LapsesAtNextSeal,
                    Reason::OpenFor { seals: 1 },
                ]
            ),
            (
                gear,
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 2
                    },
                    Reason::OpenFor { seals: 2 },
                ]
            ),
            (
                knives,
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 4
                    },
                    Reason::OpenFor { seals: 2 },
                ]
            ),
            (
                card,
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 4
                    },
                    Reason::OpenFor { seals: 2 },
                ]
            ),
            // Starved from a commitment to a task since done, from the latest
            // one, and from the root's sealing review with none.
            (swim, 4, vec![Reason::Starved { seals: 1 }]),
            (read, 4, vec![Reason::Starved { seals: 1 }]),
            (cook, 4, vec![Reason::Starved { seals: 0 }]),
            (swim_habit, 5, vec![Reason::New]),
        ]
    );
    // The tasks dormant through the deferred goal are in no tier.
    assert!(row(&db, passport).is_none());
    // A standing commitment ranks in tier 1 and keeps its goal out of tier 4.
    let walk = db.one(item("Task", "Walk to the pool"));
    db.link("Enables", walk, swim);
    db.commit(1, walk);
    db.attend(parents);
    db.attend(run);
    db.attend(swim_habit);
    db.attend(swim_habit);
    db.review(3);
    assert_eq!(
        row(&db, walk),
        Some((1, vec![Reason::OpenFor { seals: 1 }]))
    );
    assert_eq!(row(&db, swim), Some((5, vec![])));
    // The rendered text accompanies each structured reason.
    let valuation = valuation(db.view(), &ValuationOptions::default()).unwrap();
    assert_eq!(valuation.boundary, db.view().id());
    for row in &valuation.ranked {
        for reason in &row.reasons {
            assert_eq!(reason.text, reason.reason.text());
        }
    }
}

trait InconsistencyKind {
    fn ty_is_inconsistency(&self) -> bool;
}
impl InconsistencyKind for PlanningMessage {
    fn ty_is_inconsistency(&self) -> bool {
        lifecore::analysis::InnerMessageType::ty(self) == MessageType::Inconsistency
    }
}

// Acceptance tests 13 and 25: deadlines.
#[test]
fn a_task_deadline_comes_from_goals_and_projects_in_play() {
    let day = |n: i64| at(n * DAY);
    let mut db = Db::new();
    let goal = db.one(dated("Goal", "Swim", day(60)));
    let project = db.one(dated("Project", "Lessons", day(50)));
    let task = db.one(item("Task", "Book lessons"));
    db.link("Enables", task, project);
    db.link("Enables", project, goal);
    // The project is dormant, so its nearer deadline does not count; the
    // commitment keeps the task in play under it.
    db.defer(12, project);
    let late = db.one(dated("Goal", "Renew passport", day(30)));
    let form = db.one(item("Task", "Fill the form"));
    db.link("Enables", form, late);
    let (mut commitment, _) = db.commit(1, task);
    let (mut other, _) = db.commit(1, form);
    let mut deadlines = vec![];
    for (number, at) in [day(4), day(11), day(18), day(32)].into_iter().enumerate() {
        db.one(review_at(3, at));
        deadlines.push(row(&db, task).unwrap().1);
        if number < 3 {
            commitment = db.redecide(commitment, "committed", &[("cost", 1)], task).0;
            other = db.redecide(other, "committed", &[("cost", 1)], form).0;
        }
    }
    // No estimate with one review; then the median of the increasing pairs.
    assert_eq!(deadlines[0], vec![Reason::OpenFor { seals: 1 }]);
    assert_eq!(
        deadlines[1],
        vec![
            Reason::OpenFor { seals: 2 },
            Reason::DeadlineIn { reviews: 7 }
        ]
    );
    // From day 32 to day 60 over windows of 7, 7, and 14 days.
    assert_eq!(
        deadlines[3],
        vec![
            Reason::OpenFor { seals: 4 },
            Reason::DeadlineIn { reviews: 4 }
        ]
    );
    let tier1 = ranking(&db)
        .into_iter()
        .filter(|(_, tier, _)| *tier == 1)
        .map(|(id, _, reasons)| (id, reasons))
        .collect::<Vec<_>>();
    // A passed deadline ranks first among equals.
    assert_eq!(
        tier1,
        vec![
            (
                form,
                vec![Reason::OpenFor { seals: 4 }, Reason::DeadlinePassed]
            ),
            (
                task,
                vec![
                    Reason::OpenFor { seals: 4 },
                    Reason::DeadlineIn { reviews: 4 }
                ]
            ),
        ]
    );
    // The last window alone.
    let one = ValuationOptions {
        deadline_sample_windows: 1,
    };
    assert!(ranking_with(&db, &one).contains(&(
        task,
        1,
        vec![
            Reason::OpenFor { seals: 4 },
            Reason::DeadlineIn { reviews: 2 }
        ]
    )));
    // A review whose `at` does not increase adds no pair.
    db.one(review_at(3, day(20)));
    let rows = ranking_with(&db, &one);
    assert!(!rows.iter().any(|(_, _, reasons)| {
        reasons
            .iter()
            .any(|r| matches!(r, Reason::DeadlineIn { .. }))
    }));
}

// Acceptance test 27.
#[test]
fn age_counts_from_the_planning_root() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Passport"));
    let outside = db.one(item("Note", "Someday"));
    let leaving = db.one(item("Task", "Bike"));
    let left = db.one(revise("Note", "Bike, as a note", leaving));
    let becomes_habit = db.one(item("Task", "Run once"));
    db.commit(1, becomes_habit);
    db.review(3);
    // Promoted, it keeps its age; entering or returning to the plan starts
    // it afresh.
    let project = db.one(revise("Project", "Passport and visa", task));
    let entered = db.one(revise("Task", "Someday, now", outside));
    let returned = db.one(revise("Task", "Bike, again", left));
    for id in [project, entered, returned] {
        db.defer(2, id);
    }
    let habit = db.one(node_change(
        "Habit",
        titled("Run", Some(1)),
        Some((Existing(becomes_habit), RevisionIntent::Revise)),
    ));
    let second = db.review(3);
    assert!(judged(&db, second).contains(&GracePeriod {
        item: habit,
        review: second
    }));
    let open_for = |db: &Db, id| {
        row(db, id)
            .unwrap()
            .1
            .into_iter()
            .find_map(|reason| match reason {
            Reason::OpenFor { seals } => Some(seals),
            _ => None,
            })
    };
    assert_eq!(open_for(&db, project), Some(2));
    assert_eq!(open_for(&db, entered), Some(1));
    assert_eq!(open_for(&db, returned), Some(1));
    let third = db.review(3);
    assert!(judged(&db, third).contains(&undecided(habit, third)));
}

// ---------------------------------------------------------------------------
// Configuration, replay, and the suite

// Acceptance test 14.
#[test]
fn the_stale_review_nudge_requires_an_evaluation_time() {
    let configuration = r#"{"stale_review_after_days": 7}"#;
    let mut analyzer = planning(configuration);
    let none = EvaluationInputs::default();
    assert!(analyzer.initialize_inner(&none).is_err());
    let db = Db::new();
    assert!(analyzer.analyze_inner(db.view(), &none).is_err());
    // Unset, no evaluation time is required.
    assert!(planning("").initialize_inner(&none).is_ok());

    let mut db = Db::new();
    let nudge = |db: &Db, now: Timestamp| {
        planning(configuration)
            .analyze_inner(
                db.view(),
                &EvaluationInputs {
                    evaluation_time: Some(now),
                },
            )
            .unwrap()
            .into_iter()
            .filter(|f| matches!(f, StaleReview { .. }))
            .collect::<Vec<_>>()
    };
    assert_eq!(nudge(&db, at(0)), vec![]);
    db.one(item("Task", "Passport"));
    assert_eq!(nudge(&db, at(0)), vec![StaleReview { review: None }]);
    let review = db.one(review_at(0, at(DAY)));
    assert_eq!(nudge(&db, at(8 * DAY)), vec![]);
    assert_eq!(
        nudge(&db, at(8 * DAY + 1)),
        vec![StaleReview {
            review: Some(review)
        }]
    );

    // Through the runner: an error without the time, a nonsticky warning
    // with it.
    let mut db = Db::empty();
    let mut timed = runner_with(EvaluationInputs {
        evaluation_time: Some(at(0)),
    });
    let mut changes = vec![configure(PlanningFactory::name(), configuration)];
    changes.extend(PlanningType::ALL.map(|ty| declare(&ty.canonical())));
    changes.push(item("Task", "Passport"));
    let plan = prepare(&db.graph, &changes, &mut timed)
        .unwrap()
        .accepted()
        .unwrap();
    let output = plan.analysis().clone();
    plan.commit(&mut db.graph).unwrap();
    let warning = output.warnings().next().unwrap();
    assert!(!warning.diagnostic.sticky);
    assert!(matches!(
        runner().run(&db.graph),
        Err(LifeCoreError::AnalyzerExecution(_))
    ));
}

// Acceptance test 14.
#[test]
fn seals_before_first_sealed_review_are_not_judged() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Passport"));
    let first = db.review(3);
    let second = db.review(3);
    let configuration = format!(r#"{{"first_sealed_review": {}}}"#, second.position());
    assert_eq!(judged_with(&db, first, &configuration), vec![]);
    assert_eq!(
        judged_with(&db, second, &configuration),
        vec![undecided(task, second)]
    );
    assert_eq!(judged(&db, first), vec![undecided(task, first)]);
    // Sections 8.1 to 8.3 still judge all history.
    let change = db.stance("pursued", &[]);
    let stance = db.one(change);
    assert_eq!(judged_with(&db, stance, &configuration).len(), 1);
}

// Acceptance test 15.
#[test]
fn appending_never_changes_the_findings_at_earlier_prefixes() {
    let mut db = Db::new();
    let goal = db.one(item("Goal", "Swim"));
    let task = db.one(item("Task", "Lessons"));
    db.link("Enables", task, goal);
    let run = db.one(habit("Run", 2));
    db.review(3);
    let before = (0..=db.view().len())
        .map(|end| at_prefix(&db, end, ""))
        .collect::<Vec<_>>();
    db.commit(1, task);
    db.attend(run);
    db.one(revise("Task", "More lessons", task));
    // Two reviews in one plan are two seals; the second seals an empty
    // window.
    let [first, second] = db
        .apply(&[review_at(3, at(14 * DAY)), review_at(3, at(15 * DAY))])
        .try_into()
        .unwrap();
    for (end, findings) in before.iter().enumerate() {
        assert_eq!(&at_prefix(&db, end, ""), findings);
    }
    assert!(
        judged(&db, first)
            .iter()
            .any(|f| matches!(f, Undecided { .. }))
    );
    assert!(
        judged(&db, second)
            .iter()
            .any(|f| matches!(f, Undecided { review, .. } if *review == second))
    );
    assert!(judged(&db, second).contains(&Shortfall {
        item: run,
        review: second,
        dids: vec![],
        quota: 2,
    }));
}

// Acceptance test 18.
#[test]
fn with_typecheck_a_review_with_a_text_budget_is_rejected() {
    let db = Db::planned("");
    let review = node_change(
        "Review",
        Body::from([
            ("budget".into(), "three".into()),
            ("at".into(), at(0).into()),
        ]),
        None,
    );
    // The payload does not fit, so the planning analyzer errors at that
    // prefix (decision 53); the plan is rejected either way.
    assert!(matches!(
        prepare(&db.graph, &[review], &mut runner()),
        Err(transaction::Error::Core(LifeCoreError::AnalyzerExecution(
            _
        )))
    ));
    // Without planning in the suite, TypeCheck alone rejects it, and the
    // planning analyzer has no finding of its own about the type.
    let db = Db::with_suite(&[configure(TypeCheckFactory::name(), "")]);
    let review = node_change(
        "Review",
        Body::from([
            ("budget".into(), "three".into()),
            ("at".into(), at(0).into()),
        ]),
        None,
    );
    let (operations, output) = rejected(db.check(&[review]));
    assert!(
        output
            .errors()
            .all(|o| o.key.analyzer.name == TypeCheckFactory::name())
    );
    assert_eq!(operations.len(), 1);
}

// The inconsistency gate, end to end: a seal that leaves a ready task
// undecided is the transaction that fails.
#[test]
fn a_seal_inconsistency_rejects_the_plan_that_appends_the_review() {
    let mut db = Db::planned("");
    let task = db.one(item("Task", "Passport"));
    let review = db.review_change(3);
    let (operations, output) = rejected(db.check(std::slice::from_ref(&review)));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[undecided(task, operations[0])])
    );
    let (commitment, _) = db.commit(1, task);
    db.apply(&[review]);
    assert!(!runner().run(&db.graph).unwrap().has_error());
    // A seal warning is a single sticky observation.
    db.redecide(commitment, "committed", &[("cost", 1)], task);
    let run = db.one(habit("Run", 1));
    let review = db.review(3);
    let output = runner().run(&db.graph).unwrap();
    let grace = output.warnings().next().unwrap();
    assert_eq!(
        grace.key.analyzer_certificate.0,
        certificate(&GracePeriod { item: run, review })
    );
    assert!(grace.diagnostic.sticky);
    assert!(
        matches!(grace.evaluated_view, EvaluatedView::Prefixes { first, last } if first == last)
    );
    assert_eq!(grace.diagnostic.subjects, [run, review].into());
}

// ---------------------------------------------------------------------------
// PLANNING-EXPLAINER, replayed

// The explainer's database is addressed by position.
fn id_at(db: &Db, position: usize) -> EntryId {
    db.view().at(position).unwrap().id()
}
fn ts(text: &str) -> Payload {
    Payload::Timestamp(text.parse().unwrap())
}
fn body(fields: &[(&str, Payload)]) -> Body {
    fields
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect()
}
fn root(ty: &str, fields: &[(&str, Payload)]) -> Change {
    node_change(ty, body(fields), None)
}
fn after(ty: &str, of: Reference, intent: RevisionIntent, fields: &[(&str, Payload)]) -> Change {
    node_change(ty, body(fields), Some((of, intent)))
}
fn positions(ids: &[EntryId]) -> Vec<usize> {
    ids.iter().map(|id| id.position()).collect()
}
fn title(text: &str) -> (&'static str, Payload) {
    ("title", text.into())
}

// The walk of PLANNING-EXPLAINER sections 3 to 16, entry by entry, under
// TypeCheck and Planning, at the positions the explainer shows. Each file
// the explainer checks without applying is a rejection here, with the
// findings it names.
#[test]
fn the_explainer_walk_seals_as_documented() {
    use RevisionIntent::{Branch, Revise};
    let mut db = Db::empty();
    let mut changes = vec![
        configure(TypeCheckFactory::name(), ""),
        configure(PlanningFactory::name(), ""),
    ];
    use PlanningType::*;
    for ty in [
        Goal, Concern, Project, Task, Habit, External, Done, Did, Review, Stance,
    ] {
        changes.push(declare(&ty.canonical()));
    }
    db.apply(&changes);
    let x = |db: &Db, position| Existing(id_at(db, position));
    let committed = |at: &str, cost: i64| {
        root(
            "Stance",
            &[
                ("kind", "committed".into()),
                ("at", ts(at)),
                ("cost", cost.into()),
            ],
        )
    };
    let review =
        |budget: i64, at: &str| root("Review", &[("budget", budget.into()), ("at", ts(at))]);
    let event = |ty: &str, at: &str| root(ty, &[("at", ts(at))]);

    // 3: a first plan, then the review that seals it.
    let ids = db.apply(&[
        root("Goal", &[title("Swim 1 km")]),
        root("Task", &[title("Book lessons")]),
        root("Task", &[title("Attend 8 lessons")]),
        edge("Enables", Added(1), Added(2)),
        edge("Enables", Added(2), Added(0)),
        committed("2026-01-04T18:05:00Z", 1),
        edge("About", Added(5), Added(1)),
    ]);
    assert_eq!(positions(&ids), (10..=16).collect::<Vec<_>>());
    let review_1 = db.one(review(3, "2026-01-05T09:00:00Z"));
    assert_eq!(review_1.position(), 17);
    quiet(&db, review_1);
    assert_eq!(
        ranking(&db),
        vec![
            (id_at(&db, 11), 1, vec![Reason::OpenFor { seals: 1 }]),
            (id_at(&db, 10), 5, vec![]),
            (id_at(&db, 12), 5, vec![]),
        ]
    );

    // 4: a `Done` about the attendance while booking is open is rejected at
    // its `About` edge; without the commitment at 20, the seal would be.
    let (ops, output) = rejected(db.check(&[
        event("Done", "2026-01-08T17:30:00Z"),
        edge("About", Added(0), x(&db, 12)),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[PrematureCompletion {
            edge: ops[1],
            completion: crate::Completion::Done,
            sources: vec![id_at(&db, 11)],
        }])
    );
    let (ops, output) = rejected(db.check(&[
        event("Done", "2026-01-08T17:30:00Z"),
        edge("About", Added(0), x(&db, 11)),
        review(3, "2026-01-12T09:00:00Z"),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[undecided(id_at(&db, 12), ops[2])])
    );
    let ids = db.apply(&[
        event("Done", "2026-01-08T17:30:00Z"),
        edge("About", Added(0), x(&db, 11)),
        committed("2026-01-11T18:00:00Z", 2),
        edge("About", Added(2), x(&db, 12)),
    ]);
    assert_eq!(positions(&ids), [18, 19, 20, 21]);
    let review_2 = db.one(review(3, "2026-01-12T09:00:00Z"));
    assert_eq!(review_2.position(), 22);
    quiet(&db, review_2);

    // 5: editing the task drops its connections; sealing now fails twice and
    // names each dropped connection.
    let attend_10 = db.one(after(
        "Task",
        x(&db, 12),
        Revise,
        &[title("Attend 10 lessons")],
    ));
    assert_eq!(attend_10.position(), 23);
    quiet(&db, attend_10);
    let (ops, output) = rejected(db.check(&[review(3, "2026-01-13T09:00:00Z")]));
    let review_3 = ops[0];
    assert_eq!(
        inconsistencies(&output),
        certificates(&[
            undecided(attend_10, review_3),
            BrokenPlan {
                item: id_at(&db, 10),
                review: review_3,
            },
        ])
    );
    let dropped = |edge: usize, label| DroppedConnection {
        edge: id_at(&db, edge),
        label,
        superseded: vec![Superseded {
            revision: id_at(&db, 12),
            successors: vec![attend_10],
        }],
        review: review_3,
    };
    assert_eq!(
        warnings(&output),
        certificates(&[
            dropped(13, Label::Enables),
            dropped(14, Label::Enables),
            dropped(21, Label::About),
        ])
    );
    // Reconnect's three suggestions, applied one after another.
    for (label, from, to, position) in [
        ("Enables", 11, 23, 24),
        ("Enables", 23, 10, 25),
        ("About", 20, 23, 26),
    ] {
        let id = db.one(edge(label, x(&db, from), x(&db, to)));
        assert_eq!(id.position(), position);
        quiet(&db, id);
    }
    // Re-pointing the stance to an unrelated task would be rejected.
    let (ops, output) = rejected(db.check(&[
        root("Task", &[title("Something else")]),
        edge("About", x(&db, 20), Added(0)),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[UnrelatedRepoint {
            edge: ops[1],
            label: Label::About,
        }])
    );

    // 6: a split, its edges, the re-pointed commitment, and a deferral.
    let ids = db.apply(&[
        after("Task", x(&db, 23), Revise, &[title("Six club lessons")]),
        after("Task", x(&db, 23), Branch, &[title("Four private lessons")]),
    ]);
    assert_eq!(positions(&ids), [27, 28]);
    let ids = db.apply(&[
        edge("Enables", x(&db, 11), x(&db, 27)),
        edge("Enables", x(&db, 27), x(&db, 10)),
        edge("Enables", x(&db, 11), x(&db, 28)),
        edge("Enables", x(&db, 28), x(&db, 10)),
        edge("About", x(&db, 20), x(&db, 27)),
        root(
            "Stance",
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-14T20:00:00Z")),
                ("horizon", 4.into()),
            ],
        ),
        edge("About", Added(5), x(&db, 28)),
    ]);
    assert_eq!(positions(&ids), (29..=35).collect::<Vec<_>>());
    for id in ids {
        quiet(&db, id);
    }

    // 7: too much on the plate, then one decision changed.
    let ids = db.apply(&[
        root("Goal", &[title("Cycling trip in May")]),
        root("Task", &[title("Renew passport")]),
        edge("Enables", Added(1), Added(0)),
        root("Task", &[title("Fix the bike")]),
        edge("Enables", Added(3), Added(0)),
        after(
            "Stance",
            x(&db, 20),
            Revise,
            &[
                ("kind", "committed".into()),
                ("at", ts("2026-01-18T18:00:00Z")),
                ("cost", 2.into()),
            ],
        ),
        edge("About", Added(5), x(&db, 27)),
        committed("2026-01-18T18:01:00Z", 2),
        edge("About", Added(7), Added(1)),
        committed("2026-01-18T18:02:00Z", 2),
        edge("About", Added(9), Added(3)),
    ]);
    assert_eq!(positions(&ids), (36..=46).collect::<Vec<_>>());
    let (ops, output) = rejected(db.check(&[review(5, "2026-01-19T09:00:00Z")]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[BudgetOverrun {
            review: ops[0],
            stances: vec![id_at(&db, 41), id_at(&db, 43), id_at(&db, 45)],
            cost: 6,
            budget: 5,
        }])
    );
    assert_eq!(warnings(&output), BTreeSet::new());
    let ids = db.apply(&[
        after(
            "Stance",
            x(&db, 45),
            Revise,
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-19T09:10:00Z")),
                ("horizon", 1.into()),
                ("reason", "lessons first".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 39)),
    ]);
    assert_eq!(positions(&ids), [47, 48]);
    let review_3 = db.one(review(5, "2026-01-19T09:15:00Z"));
    assert_eq!(review_3.position(), 49);
    quiet(&db, review_3);

    // 8: deferring, and deferring again; the frontier is empty.
    let ids = db.apply(&[
        after(
            "Stance",
            x(&db, 43),
            Revise,
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-25T20:00:00Z")),
                ("horizon", 1.into()),
            ],
        ),
        edge("About", Added(0), x(&db, 37)),
        event("Done", "2026-01-25T20:05:00Z"),
        edge("About", Added(2), x(&db, 27)),
    ]);
    assert_eq!(positions(&ids), [50, 51, 52, 53]);
    let review_4 = db.one(review(5, "2026-01-26T09:00:00Z"));
    assert_eq!(review_4.position(), 54);
    quiet(&db, review_4);
    assert!(sealed_at(&db, review_4).frontier().is_empty());
    // A year's deferral exceeds the cap.
    let (ops, output) = rejected(db.check(&[
        after(
            "Stance",
            x(&db, 50),
            Revise,
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-26T20:00:00Z")),
                ("horizon", 52.into()),
                ("reason", "someday".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 37)),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[HorizonOverCap {
            stance: ops[0],
            horizon: 52,
            cap: 12,
        }])
    );
    let ids = db.apply(&[
        root(
            "Stance",
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-27T19:00:00Z")),
                ("horizon", 3.into()),
                ("reason", "not before March".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 36)),
    ]);
    assert_eq!(positions(&ids), [55, 56]);

    // 9: work that helps is owed a decision; a cycle is rejected.
    let ids = db.apply(&[
        root("Goal", &[title("Live abroad a year")]),
        root("Project", &[title("Sabbatical")]),
        root("Task", &[title("Language certificate")]),
        edge("Enables", Added(1), Added(0)),
        edge("Helps", Added(2), Added(1)),
    ]);
    assert_eq!(positions(&ids), (57..=61).collect::<Vec<_>>());
    let (ops, output) = rejected(db.check(&[edge("Helps", x(&db, 58), x(&db, 59))]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[Cycle {
            edge: ops[0],
            component: lifecore_dag_check::CyclicComponent {
                nodes: vec![id_at(&db, 58), id_at(&db, 59)],
                edges: vec![id_at(&db, 61), ops[0]],
            },
        }])
    );
    let ids = db.apply(&[
        root(
            "Stance",
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-01-27T19:30:00Z")),
                ("horizon", 6.into()),
                ("reason", "after the move".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 59)),
    ]);
    assert_eq!(positions(&ids), [62, 63]);

    // 10: waiting on the world. Without the wait, the seal fails three times.
    let visa_and_flights = [
        root("External", &[title("Visa decision")]),
        root("Task", &[title("Book flights")]),
        edge("Enables", Added(0), Added(1)),
        edge("Enables", Added(1), x(&db, 58)),
    ];
    let mut without_wait = visa_and_flights.to_vec();
    without_wait.push(review(5, "2026-02-02T09:00:00Z"));
    let (ops, output) = rejected(db.check(&without_wait));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[
            undecided(ops[0], ops[4]),
            BrokenPlan {
                item: id_at(&db, 57),
                review: ops[4],
            },
            BrokenPlan {
                item: id_at(&db, 58),
                review: ops[4],
            },
        ])
    );
    let mut changes = visa_and_flights.to_vec();
    changes.extend([
        root(
            "Stance",
            &[
                ("kind", "waiting".into()),
                ("at", ts("2026-01-28T19:00:00Z")),
                ("horizon", 2.into()),
            ],
        ),
        edge("About", Added(4), Added(0)),
    ]);
    let ids = db.apply(&changes);
    assert_eq!(positions(&ids), (64..=69).collect::<Vec<_>>());
    let review_5 = db.one(review(5, "2026-02-02T09:00:00Z"));
    assert_eq!(review_5.position(), 70);
    quiet(&db, review_5);

    // 11: habits and concerns, their grace period, and the first shortfall.
    let ids = db.apply(&[
        root("Goal", &[title("Finish the 10 km")]),
        root("Habit", &[title("Run"), ("quota", 3.into())]),
        edge("Helps", Added(1), Added(0)),
        root("Concern", &[title("Parents"), ("quota", 1.into())]),
    ]);
    assert_eq!(positions(&ids), (71..=74).collect::<Vec<_>>());
    let review_6 = db.one(review(5, "2026-02-09T09:00:00Z"));
    assert_eq!(review_6.position(), 75);
    let (run, parents) = (id_at(&db, 72), id_at(&db, 74));
    assert_eq!(
        judged(&db, review_6),
        vec![
            GracePeriod {
                item: run,
                review: review_6,
            },
            GracePeriod {
                item: parents,
                review: review_6,
            },
        ]
    );
    let ids = db.apply(&[
        event("Did", "2026-02-10T07:00:00Z"),
        edge("About", Added(0), x(&db, 72)),
        event("Did", "2026-02-12T07:00:00Z"),
        edge("About", Added(2), x(&db, 72)),
        event("Did", "2026-02-14T18:00:00Z"),
        edge("About", Added(4), x(&db, 74)),
    ]);
    assert_eq!(positions(&ids), (76..=81).collect::<Vec<_>>());
    let review_7 = db.one(review(5, "2026-02-16T09:00:00Z"));
    assert_eq!(review_7.position(), 82);
    assert_eq!(
        judged(&db, review_7),
        vec![Shortfall {
            item: run,
            review: review_7,
            dids: vec![id_at(&db, 76), id_at(&db, 78)],
            quota: 3,
        }]
    );
    let ids = db.apply(&[
        root(
            "Stance",
            &[
                ("kind", "deferred".into()),
                ("at", ts("2026-02-16T19:00:00Z")),
                ("horizon", 4.into()),
                ("reason", "knee".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 72)),
    ]);
    assert_eq!(positions(&ids), [83, 84]);
    assert!(open_window(&db).is_dormant(run));
    let ids = db.apply(&[
        after(
            "Stance",
            x(&db, 83),
            Revise,
            &[
                ("kind", "resumed".into()),
                ("at", ts("2026-02-17T08:00:00Z")),
                ("reason", "only bruised".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 72)),
    ]);
    assert_eq!(positions(&ids), [85, 86]);
    assert!(open_window(&db).in_play(run));
    let (ops, output) = rejected(db.check(&[
        root(
            "Stance",
            &[
                ("kind", "resumed".into()),
                ("at", ts("2026-02-17T08:01:00Z")),
            ],
        ),
        edge("About", Added(0), x(&db, 72)),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[SecondCurrentStance {
            edge: ops[1],
            current: id_at(&db, 85),
        }])
    );

    // 12: the visa arrives, the flights are ready, and the project is done
    // only once its last prerequisite is.
    let ids = db.apply(&[
        root(
            "Done",
            &[
                ("at", ts("2026-02-17T11:00:00Z")),
                ("note", "Granted for twelve months".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 64)),
    ]);
    assert_eq!(positions(&ids), [87, 88]);
    assert_eq!(row(&db, id_at(&db, 65)), Some((0, vec![Reason::Undecided])));
    let ids = db.apply(&[
        root("Task", &[title("Sublet the flat")]),
        edge("Enables", Added(0), x(&db, 58)),
        root("Task", &[title("Hand over at work")]),
        edge("Enables", Added(2), x(&db, 58)),
        event("Done", "2026-02-19T12:00:00Z"),
        edge("About", Added(4), x(&db, 65)),
        event("Done", "2026-02-21T12:00:00Z"),
        edge("About", Added(6), Added(0)),
    ]);
    assert_eq!(positions(&ids), (89..=96).collect::<Vec<_>>());
    let (ops, output) = rejected(db.check(&[
        event("Done", "2026-02-22T12:00:00Z"),
        edge("About", Added(0), x(&db, 58)),
    ]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[PrematureCompletion {
            edge: ops[1],
            completion: crate::Completion::Done,
            sources: vec![id_at(&db, 91)],
        }])
    );
    let ids = db.apply(&[
        event("Done", "2026-02-27T17:00:00Z"),
        edge("About", Added(0), x(&db, 91)),
        root(
            "Done",
            &[
                ("at", ts("2026-02-28T10:00:00Z")),
                ("note", "Leaving on the first".into()),
            ],
        ),
        edge("About", Added(2), x(&db, 58)),
    ]);
    assert_eq!(positions(&ids), (97..=100).collect::<Vec<_>>());
    // Sealed now, the ready goal, the lapsed deferral of the private
    // lessons, and the habit and concern with no `Did` would be undecided.
    let (ops, output) = rejected(db.check(&[review(5, "2026-03-02T09:00:00Z")]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[
            undecided(id_at(&db, 28), ops[0]),
            undecided(id_at(&db, 57), ops[0]),
            undecided(run, ops[0]),
            undecided(parents, ops[0]),
        ])
    );
    let ids = db.apply(&[
        root(
            "Stance",
            &[
                ("kind", "sufficient".into()),
                ("at", ts("2027-03-01T10:00:00Z")),
            ],
        ),
        edge("About", Added(0), x(&db, 57)),
    ]);
    assert_eq!(positions(&ids), [101, 102]);

    // 13 and 14: a goal with no plan goes straight to the top of the
    // ranking, beside what the next seal would find undecided.
    let cook = db.one(root("Goal", &[title("Learn to cook")]));
    assert_eq!(cook.position(), 103);
    assert_eq!(
        ranking(&db),
        vec![
            (parents, 0, vec![Reason::Undecided]),
            (cook, 0, vec![Reason::BrokenPlan]),
            (id_at(&db, 28), 0, vec![Reason::Undecided]),
            (run, 0, vec![Reason::Undecided]),
            (
                id_at(&db, 36),
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 1,
                    },
                    Reason::LapsesAtNextSeal,
                    Reason::OpenFor { seals: 5 },
                ]
            ),
            (
                id_at(&db, 59),
                3,
                vec![
                    Reason::Standing {
                        kind: StanceKind::Deferred,
                        seals_left: 4,
                    },
                    Reason::OpenFor { seals: 3 },
                ]
            ),
            (id_at(&db, 10), 4, vec![Reason::Starved { seals: 4 }]),
            (id_at(&db, 71), 4, vec![Reason::Starved { seals: 1 }]),
        ]
    );
    let ids = db.apply(&[
        root(
            "Stance",
            &[
                ("kind", "abandoned".into()),
                ("at", ts("2027-03-02T19:00:00Z")),
                ("reason", "Knee".into()),
            ],
        ),
        edge("About", Added(0), x(&db, 71)),
    ]);
    assert_eq!(positions(&ids), [104, 105]);
    // Run helped only the abandoned goal, so it serves nothing and stands
    // on its own.
    assert!(open_window(&db).in_play(run));
    assert!(
        crate::current::Current::new(db.view())
            .unwrap()
            .serves(run)
            .is_empty()
    );

    // 16: the user's own field, label, and type are not planning's.
    let mut task = PlanningType::Task.canonical();
    task.fields.insert(
        "place".into(),
        lifecore::typing::FieldDecl {
            status: lifecore::typing::FieldStatus::Optional,
            shape: lifecore::typing::ValueType::Text,
        },
    );
    task.outgoing.insert("Cites".into());
    let ids = db.apply(&[
        revise_declaration(&task, id_at(&db, 3)),
        declare(&Type::new(
            "Book",
            &[(
                "title",
                (
                    lifecore::typing::ValueType::Text,
                    lifecore::typing::FieldStatus::Mandatory,
                ),
            )],
            &[],
            &["Cites"],
            &[],
        )),
        root("Book", &[title("Road bike repair manual")]),
        edge("Cites", x(&db, 39), Added(2)),
    ]);
    assert_eq!(positions(&ids), (106..=109).collect::<Vec<_>>());
    for id in ids {
        quiet(&db, id);
    }
    let (ops, output) = rejected(db.check(&[edge("About", x(&db, 39), x(&db, 108))]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[bad_endpoints(ops[0], Label::About, "Task", "Book")])
    );
    let mut no_budget = PlanningType::Review.canonical();
    no_budget.fields.get_mut("budget").unwrap().status = lifecore::typing::FieldStatus::Optional;
    let (ops, output) = rejected(db.check(&[revise_declaration(&no_budget, id_at(&db, 8))]));
    assert_eq!(
        inconsistencies(&output),
        certificates(&[UnmetRequirement {
            declaration: ops[0],
            name: PlanningType::Review,
            requirement: Requirement::Field {
                field: "budget",
                rule: FieldRule::MandatoryInteger,
            },
        }])
    );

    // The whole walk passes, with the warnings the explainer shows.
    let output = runner().run(&db.graph).unwrap();
    assert!(!output.has_error());
    assert_eq!(
        warnings(&output),
        certificates(&[
            GracePeriod {
                item: run,
                review: review_6,
            },
            GracePeriod {
                item: parents,
                review: review_6,
            },
            Shortfall {
                item: run,
                review: review_7,
                dids: vec![id_at(&db, 76), id_at(&db, 78)],
                quota: 3,
            },
        ])
    );
}

// Every variant converts: its certificate is its JSON encoding, its focus is
// one of its subjects, and its category and stickiness are the rule's.
#[test]
fn every_finding_converts_to_a_message() {
    let db = Db::new();
    let id = |position| id_at(&db, position);
    let (a, b, c) = (id(0), id(1), id(2));
    let component = lifecore_dag_check::CyclicComponent {
        nodes: vec![a, b],
        edges: vec![c],
    };
    let inconsistencies = vec![
        UnknownKind {
            stance: a,
            kind: "pursued".into(),
        },
        MissingCost { stance: a },
        MissingHorizon {
            stance: a,
            kind: StanceKind::Waiting,
        },
        ForbiddenField {
            stance: a,
            kind: StanceKind::Resumed,
            field: "cost",
        },
        NegativeCost {
            stance: a,
            cost: -1,
        },
        HorizonBelowOne {
            stance: a,
            horizon: 0,
        },
        HorizonOverCap {
            stance: a,
            horizon: 13,
            cap: 12,
        },
        NegativeBudget {
            review: a,
            budget: -1,
        },
        NegativeQuota { item: a, quota: -1 },
        RevisedBookkeeping {
            node: a,
            ty: PlanningType::Did,
        },
        CrossesBookkeeping {
            node: a,
            from: PlanningType::Task,
            to: PlanningType::Stance,
        },
        SuccessorOfTerminal {
            node: a,
            predecessor: b,
            state: ItemState::Done,
        },
        SuccessorOfFinalStance {
            node: a,
            predecessor: b,
            kind: StanceKind::Abandoned,
        },
        UnmetRequirement {
            declaration: a,
            name: PlanningType::Review,
            requirement: Requirement::NoSuccessors,
        },
        bad_endpoints(a, Label::About, "Note", "Task"),
        TerminalTarget {
            edge: a,
            label: Label::Helps,
            state: ItemState::Abandoned,
        },
        Cycle { edge: c, component },
        KindDoesNotApply {
            edge: a,
            kind: StanceKind::Committed,
            target_type: PlanningType::Goal,
        },
        UntilFromNonDeferral {
            edge: a,
            kind: StanceKind::Waiting,
        },
        SecondUntil {
            edge: a,
            current: b,
        },
        SecondDoneAbout {
            edge: a,
            earlier: b,
        },
        AboutTerminal {
            edge: a,
            state: ItemState::Sufficient,
        },
        PrematureCompletion {
            edge: a,
            completion: Completion::Abandoned,
            sources: vec![b],
        },
        UnrelatedRepoint {
            edge: a,
            label: Label::Until,
        },
        SecondCurrentStance {
            edge: a,
            current: b,
        },
        undecided(a, b),
        BudgetOverrun {
            review: a,
            stances: vec![b, c],
            cost: 3,
            budget: 2,
        },
        BrokenPlan { item: a, review: b },
        BlockedByAbandoned {
            edge: a,
            source: b,
            target: c,
            review: id(3),
        },
    ];
    let warnings = vec![
        Shortfall {
            item: a,
            review: b,
            dids: vec![c],
            quota: 2,
        },
        GracePeriod { item: a, review: b },
        DroppedConnection {
            edge: a,
            label: Label::About,
            superseded: vec![Superseded {
                revision: b,
                successors: vec![c],
            }],
            review: id(3),
        },
        StaleReview { review: Some(a) },
        StaleReview { review: None },
    ];
    let analyzer = planning("");
    let check = |finding: PlanningMessage, ty: MessageType| {
        let message = analyzer.convert_message(finding.clone());
        assert_eq!(message.analyzer_certificate.0, certificate(&finding));
        assert_eq!(message.ty(), ty, "{finding:?}");
        assert_eq!(message.diagnostic.subjects, finding.subjects());
        if let Some(focus) = message.diagnostic.focus {
            assert!(message.diagnostic.subjects.contains(&focus), "{finding:?}");
        }
        assert!(!message.diagnostic.message.is_empty());
        message.diagnostic.sticky
    };
    for finding in inconsistencies {
        assert!(!check(finding, MessageType::Inconsistency));
    }
    let sticky = warnings
        .into_iter()
        .map(|finding| check(finding, MessageType::Warning))
        .collect::<Vec<_>>();
    assert_eq!(sticky, [true, true, true, false, false]);
    assert!(!check(
        Bootstrap {
            name: PlanningType::Goal
        },
        MessageType::Suggestion
    ));
}

// Costs, horizons, and quotas at the ends of their range neither overflow nor
// panic.
#[test]
fn extreme_integers_are_judged_without_overflow() {
    let mut db = Db::new();
    let a = db.one(item("Task", "a"));
    let b = db.one(item("Task", "b"));
    let (sa, _) = db.commit(i64::MAX, a);
    let (sb, _) = db.commit(i64::MAX, b);
    let review = db.one(review_at(i64::MAX, at(DAY)));
    assert_eq!(
        judged(&db, review),
        vec![BudgetOverrun {
            review,
            stances: vec![sa, sb],
            cost: 2 * i128::from(i64::MAX),
            budget: i64::MAX,
        }]
    );
    let task = db.one(item("Task", "Someday"));
    db.defer(i64::MAX, task);
    let lots = db.one(habit("Run", i64::MAX));
    let none = db.one(habit("Swim", i64::MIN));
    db.one(review_at(0, at(2 * DAY)));
    let review = db.one(review_at(0, at(3 * DAY)));
    let uncapped = r#"{"max_deferral_horizon": null}"#;
    let findings = judged_with(&db, review, uncapped);
    assert!(findings.contains(&Shortfall {
        item: lots,
        review,
        dids: vec![],
        quota: i64::MAX,
    }));
    assert!(findings.contains(&undecided(none, review)));
    let rows = ranking(&db);
    assert!(rows.contains(&(
        task,
        3,
        vec![
            Reason::Standing {
                kind: StanceKind::Deferred,
                seals_left: (i64::MAX as usize) - 1,
            },
            Reason::OpenFor { seals: 2 },
        ]
    )));
}

// Decision 82: a deferral whose `Until` target was revised in the open
// window, and not re-pointed, lapses after the next seal, and the ranking
// says so before that seal.
#[test]
fn an_unanswered_until_revision_leaves_one_seal() {
    let mut db = Db::new();
    let task = db.one(item("Task", "Certificate"));
    let waits_on = db.one(item("External", "Visa"));
    let (deferral, _) = db.defer(3, task);
    db.link("Until", deferral, waits_on);
    db.review(3);
    let standing = |seals_left| {
        let mut reasons = vec![Reason::Standing {
            kind: StanceKind::Deferred,
            seals_left,
        }];
        if seals_left == 1 {
            reasons.push(Reason::LapsesAtNextSeal);
        }
        reasons.push(Reason::OpenFor { seals: 1 });
        Some((3, reasons))
    };
    assert_eq!(row(&db, task), standing(3));
    let revised = db.one(revise("External", "Visa, appealed", waits_on));
    assert_eq!(row(&db, task), standing(1));
    // Re-pointed before the seal, the deferral keeps its clock.
    let mut repointed = db.fork();
    repointed.link("Until", deferral, revised);
    assert_eq!(row(&repointed, task), standing(3));
    // Unanswered, it lapses after that seal, as the row said.
    let seal = db.review(3);
    assert!(sealed_at(&db, seal).is_dormant(task));
    assert_eq!(row(&db, task), Some((0, vec![Reason::Undecided])));
}

// Decision 83: two connections a seal does not ask to carry.
#[test]
fn a_dropped_connection_skips_an_ended_until_and_an_earlier_did() {
    // A deferral ended by a commitment loses its `Until` edge for good; one
    // renewed as a deferral must carry it.
    let mut db = Db::new();
    let task = db.one(item("Task", "Certificate"));
    let waits_on = db.one(item("External", "Visa"));
    db.decide("waiting", &[("horizon", 9)], waits_on);
    let (deferral, _) = db.defer(3, task);
    let until = db.link("Until", deferral, waits_on);
    db.review(3);
    let dropped_until = |db: &Db, review: EntryId| {
        judged(db, review)
            .iter()
            .any(|f| matches!(f, DroppedConnection { edge, .. } if *edge == until))
    };
    let mut ended = db.fork();
    ended.redecide(deferral, "committed", &[("cost", 1)], task);
    let review = ended.review(3);
    assert!(!dropped_until(&ended, review));
    let mut renewed = db.fork();
    renewed.redecide(deferral, "deferred", &[("horizon", 2)], task);
    let review = renewed.review(3);
    assert!(dropped_until(&renewed, review));

    // Revising a habit asks only for this window's `Did` events.
    let mut db = Db::new();
    let run = db.one(habit("Run", 1));
    let (_, earlier) = db.attend(run);
    db.review(3);
    let (_, this_window) = db.attend(run);
    let jog = db.one(node_change(
        "Habit",
        titled("Jog", Some(1)),
        Some((Existing(run), RevisionIntent::Revise)),
    ));
    let review = db.review(3);
    let dropped = judged(&db, review)
        .into_iter()
        .filter_map(|f| match f {
        DroppedConnection { edge, .. } => Some(edge),
        _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!dropped.contains(&earlier));
    assert_eq!(dropped, vec![this_window]);
    assert!(judged(&db, review).contains(&undecided(jog, review)));
}
