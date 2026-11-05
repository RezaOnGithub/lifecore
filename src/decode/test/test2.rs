// Decode and declaration types of the basic shape of PLANNING's vocabulary
// (PLANNING §3 and §11 at 59c702c, as the core round's section 5.1 revises
// it), plus one type that is not Planning's, for lists, nested records,
// presence attributes, and renames. These are fixtures, not an analyzer.

use crate::{
    Body, Date, EntryId, Node, OffsetTimestamp, Payload, PayloadKind, SignedDuration, Timestamp,
    decode::{
        Declare, DecodeError, DecodeProblem, FromBody, FromPayload, PayloadError, optional_field,
    },
    typing::{FieldDecl, FieldStatus, PathPart, Type, ValueType, path_text},
};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, FromPayload)]
enum Per {
    Window,
    Day,
    Week,
}

#[derive(Clone, Debug, PartialEq, Eq, FromPayload)]
enum Decision {
    Committed { cost: SignedDuration },
    Deferred { horizon: i64 },
    Waiting { horizon: i64 },
    Resumed {},
    Sufficient {},
    Abandoned {},
}

#[derive(Declare)]
#[lifecore(type = "Goal", incoming = ["Enables", "Helps", "About"])]
#[allow(dead_code)]
struct GoalDeclaration {
    title: String,
    criterion: Option<String>,
    deadline: Option<Date>,
}

#[derive(Debug, PartialEq, FromBody)]
struct Goal {
    title: String,
    deadline: Option<Date>,
}

#[derive(Declare)]
#[lifecore(type = "Habit", outgoing = ["Helps"], incoming = ["About"])]
#[allow(dead_code)]
struct HabitDeclaration {
    title: String,
    quota: i64,
    per: Per,
}

#[derive(Debug, PartialEq, FromBody)]
struct Habit {
    quota: i64,
    per: Per,
}

#[derive(Declare)]
#[lifecore(type = "Stance", outgoing = ["About", "Until"])]
#[allow(dead_code)]
struct StanceDeclaration {
    decision: Decision,
    at: OffsetTimestamp,
    reason: Option<String>,
}

// What Planning's rules read; `at` and `reason` are free (PLANNING §3.4).
#[derive(Debug, PartialEq, FromBody)]
struct Stance {
    decision: Decision,
}

#[derive(Declare)]
#[lifecore(type = "Review")]
#[allow(dead_code)]
struct ReviewDeclaration {
    budget: SignedDuration,
    at: OffsetTimestamp,
    note: Option<String>,
}

#[derive(Debug, PartialEq, FromBody)]
struct Review {
    budget: SignedDuration,
    at: OffsetTimestamp,
}

#[derive(Declare)]
#[lifecore(type = "Did", outgoing = ["About"])]
#[allow(dead_code)]
struct DidDeclaration {
    at: OffsetTimestamp,
    note: Option<String>,
}

#[derive(Debug, PartialEq, FromBody)]
struct Did {
    at: OffsetTimestamp,
}

// Not Planning's.
#[derive(Debug, PartialEq, FromBody)]
struct Step {
    label: String,
    done: bool,
    #[lifecore(rename = "took")]
    duration: Option<SignedDuration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, FromPayload)]
enum Mark {
    PlainText,
    OffsetTimestamp,
    #[lifecore(rename = "list-of")]
    ListOf,
}

#[derive(Debug, PartialEq, FromPayload)]
enum Outcome {
    // A unit variant beside tags with fields is a tag with an empty body.
    Skipped,
    #[lifecore(rename = "done")]
    Finished {
        at: Timestamp,
        #[lifecore(encouraged)]
        note: Option<String>,
    },
}

#[derive(Declare)]
#[lifecore(type = "Checklist", successors = ["Archive"], incoming = [])]
#[allow(dead_code)]
struct ChecklistDeclaration {
    title: String,
    steps: Vec<Step>,
    tags: Vec<Vec<String>>,
    r#type: Mark,
    outcome: Option<Outcome>,
    #[lifecore(encouraged)]
    due: Option<Date>,
    #[lifecore(deprecated, rename = "created")]
    created_at: Option<Timestamp>,
}

#[derive(Debug, PartialEq, FromBody)]
struct Checklist {
    steps: Vec<Step>,
    tags: Vec<Vec<String>>,
    r#type: Mark,
    outcome: Option<Outcome>,
    #[lifecore(encouraged)]
    due: Option<Date>,
}

// The declaration payloads of PLANNING §11, revised by the core round's
// section 5.1, read through the declaration codec of FILES §2.6.
const GOAL: &str = r#"
name = "Goal"
fields = { kind = "record", value = { title = ["mandatory", "text"], criterion = ["optional", "text"], deadline = ["optional", "date"] } }
outgoing = []
incoming = ["Enables", "Helps", "About"]
successors = []
"#;

const HABIT: &str = r#"
name = "Habit"
fields = { kind = "record", value = { title = ["mandatory", "text"], quota = ["mandatory", "integer"], per = ["mandatory", "enum", ["window", "day", "week"]] } }
outgoing = ["Helps"]
incoming = ["About"]
successors = []
"#;

const STANCE: &str = r#"
name = "Stance"
outgoing = ["About", "Until"]
incoming = []
successors = []

[fields]
kind = "record"

[fields.value]
at = ["mandatory", "offset_timestamp"]
reason = ["optional", "text"]
decision = ["mandatory", "variant", { kind = "record", value = {
  committed = { kind = "record", value = { cost = ["mandatory", "duration"] } },
  deferred = { kind = "record", value = { horizon = ["mandatory", "integer"] } },
  waiting = { kind = "record", value = { horizon = ["mandatory", "integer"] } },
  resumed = { kind = "record", value = {} },
  sufficient = { kind = "record", value = {} },
  abandoned = { kind = "record", value = {} },
} }]
"#;

const REVIEW: &str = r#"
name = "Review"
fields = { kind = "record", value = { budget = ["mandatory", "duration"], at = ["mandatory", "offset_timestamp"], note = ["optional", "text"] } }
outgoing = []
incoming = []
successors = []
"#;

const DID: &str = r#"
name = "Did"
fields = { kind = "record", value = { at = ["mandatory", "offset_timestamp"], note = ["optional", "text"] } }
outgoing = ["About"]
incoming = []
successors = []
"#;

const CHECKLIST: &str = r#"
name = "Checklist"
outgoing = []
incoming = []
successors = ["Archive"]

[fields]
kind = "record"

[fields.value]
title = ["mandatory", "text"]
steps = ["mandatory", "list", "record", { kind = "record", value = { label = ["mandatory", "text"], done = ["mandatory", "boolean"], took = ["optional", "duration"] } }]
tags = ["mandatory", "list", "list", "text"]
type = ["mandatory", "enum", ["plain_text", "offset_timestamp", "list-of"]]
outcome = ["optional", "variant", { kind = "record", value = {
  skipped = { kind = "record", value = {} },
  done = { kind = "record", value = { at = ["mandatory", "timestamp"], note = ["encouraged", "text"] } },
} }]
due = ["encouraged", "date"]
created = ["deprecated", "timestamp"]
"#;

fn body(toml: &str) -> Body {
    toml::from_str(toml).unwrap()
}

fn declared(toml: &str) -> Type {
    Type::from_body(&body(toml)).unwrap()
}

fn node(ty_name: &str, payload: &str) -> Node {
    Node {
        id: EntryId {
            position: 7,
            origin: Uuid::nil(),
        },
        ty_name: ty_name.into(),
        predecessor: None,
        payload: body(payload),
    }
}

// Every declaration with the TOML that owns it, and the fields its decode
// type reads.
fn fixtures() -> Vec<(Type, &'static str, BTreeMap<String, FieldDecl>)> {
    vec![
        (GoalDeclaration::declaration(), GOAL, Goal::fields()),
        (HabitDeclaration::declaration(), HABIT, Habit::fields()),
        (StanceDeclaration::declaration(), STANCE, Stance::fields()),
        (ReviewDeclaration::declaration(), REVIEW, Review::fields()),
        (DidDeclaration::declaration(), DID, Did::fields()),
        (
            ChecklistDeclaration::declaration(),
            CHECKLIST,
            Checklist::fields(),
        ),
    ]
}

// The fit of SPEC 6.5: what a decode type reads, against the declaration.
// Each unmet requirement names its place, with `[]` for a list's elements.
fn unmet(read: &BTreeMap<String, FieldDecl>, declaration: &Type) -> Vec<String> {
    let mut problems = vec![];
    body_unmet(read, &declaration.fields, "", &mut problems);
    problems
}

fn body_unmet(
    read: &BTreeMap<String, FieldDecl>,
    declared: &BTreeMap<String, FieldDecl>,
    at: &str,
    problems: &mut Vec<String>,
) {
    for (name, field) in read {
        let at = if at.is_empty() {
            name.clone()
        } else {
            format!("{at}.{name}")
        };
        match declared.get(name) {
        None => problems.push(format!("{at} is not declared")),
        Some(declared) => {
            if field.status == FieldStatus::Mandatory && declared.status != FieldStatus::Mandatory {
                problems.push(format!("{at} is read as mandatory"));
            }
            shape_unmet(&field.shape, &declared.shape, &at, problems);
        }
        }
    }
}

fn shape_unmet(read: &ValueType, declared: &ValueType, at: &str, problems: &mut Vec<String>) {
    match (read, declared) {
    (ValueType::List { element: read }, ValueType::List { element: declared }) => {
        shape_unmet(read, declared, &format!("{at}[]"), problems)
    }
    (ValueType::Record { fields: read }, ValueType::Record { fields: declared }) => {
        body_unmet(read, declared, at, problems)
    }
    (ValueType::Enum { members: read }, ValueType::Enum { members: declared }) => {
        for member in declared.difference(read) {
            problems.push(format!("{at} does not decode member {member:?}"));
        }
    }
    (ValueType::Variant { tags: read }, ValueType::Variant { tags: declared }) => {
        for (tag, declared) in declared {
            match read.get(tag) {
            None => problems.push(format!("{at} does not decode tag {tag:?}")),
            Some(read) => body_unmet(read, declared, &format!("{at}({tag})"), problems),
            }
        }
    }
    (read, declared) if read == declared => {}
    _ => problems.push(format!("{at} has another shape")),
    }
}

#[test]
fn declarations_equal_the_toml_the_tests_hold() {
    for (declaration, toml, _) in fixtures() {
        assert_eq!(declaration, declared(toml), "{}", declaration.name);
    }
}

#[test]
fn declarations_round_trip_through_the_declaration_codec() {
    for (declaration, _, _) in fixtures() {
        let payload = declaration.to_body();
        assert_eq!(Type::from_body(&payload).unwrap(), declaration);
        let json: Body = serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();
        assert_eq!(Type::from_body(&json).unwrap(), declaration);
        let toml: Body = toml::from_str(&toml::to_string(&payload).unwrap()).unwrap();
        assert_eq!(Type::from_body(&toml).unwrap(), declaration);
    }
}

#[test]
fn every_decode_type_reads_a_subset_of_its_declaration() {
    for (declaration, _, read) in fixtures() {
        assert_eq!(unmet(&read, &declaration), Vec::<String>::new());
    }
    // A field read as `Option<T>` may have any policy; the read set is the
    // fields it names, with their read policy.
    assert_eq!(Checklist::fields()["due"].status, FieldStatus::Encouraged);
    assert_eq!(Goal::fields()["deadline"].status, FieldStatus::Optional);
    assert_eq!(Goal::fields().len(), 2);
    assert_eq!(Stance::fields().keys().collect::<Vec<_>>(), ["decision"]);
}

#[test]
fn the_subset_rule_rejects_reads_the_declaration_does_not_keep() {
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Overreaching {
        criterion: String,
        deadline: Option<Timestamp>,
        horizon: Option<i64>,
    }
    assert_eq!(
        unmet(&Overreaching::fields(), &GoalDeclaration::declaration()),
        [
            "criterion is read as mandatory",
            "deadline has another shape",
            "horizon is not declared",
        ]
    );
}

// Which shapes fit is what decoding accepts: a decode type may know more
// members and tags than are declared, and read fewer fields, at any depth.
#[test]
fn a_fit_is_recursive() {
    #[derive(FromPayload)]
    #[allow(dead_code)]
    enum Wider {
        Window,
        Day,
        Week,
        Month,
    }
    #[derive(FromPayload)]
    #[allow(dead_code)]
    enum Narrower {
        Day,
        Week,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct WiderHabit {
        per: Wider,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct NarrowerHabit {
        per: Narrower,
    }
    assert_eq!(
        unmet(&WiderHabit::fields(), &HabitDeclaration::declaration()),
        Vec::<String>::new()
    );
    assert_eq!(
        unmet(&NarrowerHabit::fields(), &HabitDeclaration::declaration()),
        [r#"per does not decode member "window""#]
    );

    // Fewer fields in a tag fit; fewer tags do not.
    #[derive(FromPayload)]
    #[allow(dead_code)]
    enum Lean {
        Committed {},
        Deferred { horizon: i64 },
        Waiting {},
        Resumed {},
        Sufficient {},
        Abandoned {},
        Postponed { until: Date },
    }
    #[derive(FromPayload)]
    #[allow(dead_code)]
    enum Few {
        Committed { cost: SignedDuration },
        Deferred { horizon: Option<i64> },
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct LeanStance {
        decision: Lean,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct FewStance {
        decision: Few,
    }
    assert_eq!(
        unmet(&LeanStance::fields(), &StanceDeclaration::declaration()),
        Vec::<String>::new()
    );
    assert_eq!(
        unmet(&FewStance::fields(), &StanceDeclaration::declaration()),
        [
            r#"decision does not decode tag "abandoned""#,
            r#"decision does not decode tag "resumed""#,
            r#"decision does not decode tag "sufficient""#,
            r#"decision does not decode tag "waiting""#,
        ]
    );

    // A nested record may read fewer fields, an `Option<T>` under any
    // policy, and a field read as `T` only where it is mandatory.
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Label {
        label: String,
        #[lifecore(deprecated)]
        took: Option<SignedDuration>,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Took {
        took: SignedDuration,
        done: Option<bool>,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Labels {
        steps: Vec<Label>,
        outcome: Option<Outcome>,
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Durations {
        steps: Vec<Took>,
        tags: Vec<String>,
    }
    assert_eq!(
        unmet(&Labels::fields(), &ChecklistDeclaration::declaration()),
        Vec::<String>::new()
    );
    assert_eq!(
        unmet(&Durations::fields(), &ChecklistDeclaration::declaration()),
        [
            "steps[].took is read as mandatory",
            "tags[] has another shape"
        ]
    );
    #[derive(FromPayload)]
    #[allow(dead_code)]
    enum StrictOutcome {
        Skipped,
        #[lifecore(rename = "done")]
        Finished {
            note: String,
        },
    }
    #[derive(FromBody)]
    #[allow(dead_code)]
    struct Strict {
        outcome: Option<StrictOutcome>,
    }
    assert_eq!(
        unmet(&Strict::fields(), &ChecklistDeclaration::declaration()),
        ["outcome(done).note is read as mandatory"]
    );
}

// Types written through `macro_rules!` fragments carry that macro's hygiene,
// and a `ty` fragment reaches the derive as an invisible group.
macro_rules! body_tt {
    ($name:ident, $($t:tt)*) => {
        #[derive(Debug, PartialEq, FromBody)]
        struct $name {
            a: $($t)*
        }
    };
}
macro_rules! body_ty {
    ($name:ident, $t:ty) => {
        #[derive(Debug, PartialEq, FromBody)]
        struct $name {
            a: $t,
        }
    };
}
macro_rules! declare_ty {
    ($name:ident, $t:ty) => {
        #[derive(Declare)]
        #[lifecore(type = "Made")]
        #[allow(dead_code)]
        struct $name {
            a: $t,
        }
    };
}
macro_rules! variant_tt {
    ($name:ident, $($t:tt)*) => {
        #[derive(Debug, PartialEq, FromPayload)]
        enum $name {
            Made { a: $($t)* },
        }
    };
}
macro_rules! variant_ty {
    ($name:ident, $t:ty) => {
        #[derive(Debug, PartialEq, FromPayload)]
        enum $name {
            Made { a: $t },
        }
    };
}
body_tt!(TtInteger, i64);
body_tt!(TtOptional, Option<i64>);
body_ty!(TyInteger, i64);
body_ty!(TyOptional, Option<i64>);
declare_ty!(DeclaredInteger, i64);
declare_ty!(DeclaredOptional, Option<i64>);
variant_tt!(TtTag, Option<i64>);
variant_ty!(TyTag, Option<i64>);
variant_ty!(TyMandatoryTag, i64);

#[test]
fn derives_work_inside_macro_rules() {
    let some = body("a = 1");
    let none = body("b = 1");
    assert_eq!(TtInteger::from_body(&some), Ok(TtInteger { a: 1 }));
    assert_eq!(TyInteger::from_body(&some), Ok(TyInteger { a: 1 }));
    assert_eq!(TtOptional::from_body(&none), Ok(TtOptional { a: None }));
    assert_eq!(TyOptional::from_body(&none), Ok(TyOptional { a: None }));
    assert_eq!(TyOptional::from_body(&some), Ok(TyOptional { a: Some(1) }));
    assert_eq!(
        TyInteger::from_body(&none).unwrap_err().problem,
        DecodeProblem::Missing
    );
    let status = |fields: BTreeMap<String, FieldDecl>| fields["a"].status;
    assert_eq!(status(TtOptional::fields()), FieldStatus::Optional);
    assert_eq!(status(TyOptional::fields()), FieldStatus::Optional);
    assert_eq!(status(TyInteger::fields()), FieldStatus::Mandatory);
    assert_eq!(
        status(DeclaredOptional::declaration().fields),
        FieldStatus::Optional
    );
    assert_eq!(
        status(DeclaredInteger::declaration().fields),
        FieldStatus::Mandatory
    );
    assert_eq!(
        DeclaredOptional::declaration().fields["a"].shape,
        ValueType::Integer
    );

    let empty = Payload::Variant {
        tag: "made".into(),
        value: Body::new(),
    };
    assert_eq!(TtTag::from_payload(&empty), Ok(TtTag::Made { a: None }));
    assert_eq!(TyTag::from_payload(&empty), Ok(TyTag::Made { a: None }));
    assert_eq!(
        TyMandatoryTag::from_payload(&empty).unwrap_err().path,
        [
            PathPart::Tag { tag: "made".into() },
            PathPart::Field("a".into())
        ]
    );
}

// `crate = …` names the crate through another path; here an alias of this
// crate's root, as a dependent would name a renamed dependency.
mod renamed {
    use crate as core_db;
    use core_db::decode::{Declare, FromBody, FromPayload};

    #[derive(Debug, PartialEq, FromPayload)]
    #[lifecore(crate = "core_db")]
    pub enum Per {
        Day,
    }

    #[derive(Debug, PartialEq, FromPayload)]
    #[lifecore(crate = "core_db")]
    pub enum Decision {
        Committed { cost: core_db::SignedDuration },
    }

    #[derive(Debug, PartialEq, FromBody)]
    #[lifecore(crate = "core_db")]
    pub struct Habit {
        pub per: Per,
        pub decision: Option<Decision>,
    }

    #[derive(Declare)]
    #[lifecore(crate = "core_db", type = "Habit", incoming = ["About"])]
    #[allow(dead_code)]
    pub struct HabitDeclaration {
        per: Per,
        decision: Option<Decision>,
    }
}

#[test]
fn a_renamed_crate_path_compiles_and_decodes() {
    assert_eq!(
        node("Habit", r#"per = "day""#).decode::<renamed::Habit>(),
        Ok(renamed::Habit {
            per: renamed::Per::Day,
            decision: None
        })
    );
    let declaration = renamed::HabitDeclaration::declaration();
    assert_eq!(declaration.incoming.len(), 1);
    assert_eq!(
        unmet(&renamed::Habit::fields(), &declaration),
        Vec::<String>::new()
    );
}

#[test]
fn derived_shapes_are_the_declared_words() {
    let members = |names: &[&str]| ValueType::Enum {
        members: names.iter().map(|name| name.to_string()).collect(),
    };
    assert_eq!(Per::shape(), members(&["window", "day", "week"]));
    assert_eq!(
        Mark::shape(),
        members(&["plain_text", "offset_timestamp", "list-of"])
    );
    assert_eq!(
        Vec::<Vec<String>>::shape(),
        ValueType::List {
            element: Box::new(ValueType::List {
                element: Box::new(ValueType::Text)
            })
        }
    );
    assert_eq!(
        Step::shape(),
        ValueType::Record {
            fields: Step::fields()
        }
    );
    let ValueType::Variant { tags } = Decision::shape() else {
        panic!("a variant")
    };
    assert_eq!(
        tags.keys().collect::<Vec<_>>(),
        [
            "abandoned",
            "committed",
            "deferred",
            "resumed",
            "sufficient",
            "waiting"
        ]
    );
    assert!(tags["resumed"].is_empty());
    for (rust, shape) in [
        (String::shape(), ValueType::Text),
        (i64::shape(), ValueType::Integer),
        (bool::shape(), ValueType::Boolean),
        (Timestamp::shape(), ValueType::Timestamp),
        (SignedDuration::shape(), ValueType::Duration),
        (Date::shape(), ValueType::Date),
        (OffsetTimestamp::shape(), ValueType::OffsetTimestamp),
    ] {
        assert_eq!(rust, shape);
    }
}

#[test]
fn bodies_typecheck_accepts_decode() {
    let stance = node(
        "Stance",
        r#"
        decision = { kind = "variant", tag = "committed", value = { cost = { kind = "duration", value = "1h 30m" } } }
        at = { kind = "offset_timestamp", value = "2026-10-29T08:15:00+01:00" }
        "#,
    );
    assert_eq!(StanceDeclaration::declaration().check(&stance.payload), []);
    assert_eq!(
        stance.decode::<Stance>().unwrap(),
        Stance {
            decision: Decision::Committed {
                cost: SignedDuration::from_mins(90)
            }
        }
    );
    let habit = node(
        "Habit",
        r#"title = "Stretch"
quota = 3
per = "day""#,
    );
    assert_eq!(HabitDeclaration::declaration().check(&habit.payload), []);
    assert_eq!(
        habit.decode::<Habit>().unwrap(),
        Habit {
            quota: 3,
            per: Per::Day
        }
    );
    let review = node(
        "Review",
        r#"
        budget = { kind = "duration", value = "PT10H" }
        at = { kind = "offset_timestamp", value = "2026-11-01T18:00:00-05:00" }
        note = "after the trip"
        "#,
    );
    assert_eq!(ReviewDeclaration::declaration().check(&review.payload), []);
    assert_eq!(
        review.decode::<Review>().unwrap(),
        Review {
            budget: SignedDuration::from_hours(10),
            at: "2026-11-01T18:00:00-05:00".parse().unwrap(),
        }
    );
    let goal = node(
        "Goal",
        r#"
        title = "Ship"
        deadline = { kind = "date", value = "2026-12-24" }
        "#,
    );
    assert_eq!(GoalDeclaration::declaration().check(&goal.payload), []);
    assert_eq!(
        goal.decode::<Goal>().unwrap(),
        Goal {
            title: "Ship".into(),
            deadline: Some(Date::constant(2026, 12, 24)),
        }
    );
    let checklist = node(
        "Checklist",
        r#"
        title = "Pack"
        steps = [
          { kind = "record", value = { label = "socks", done = { kind = "boolean", value = true } } },
          { kind = "record", value = { label = "tent", done = { kind = "boolean", value = false }, took = { kind = "duration", value = "PT20M" } } },
        ]
        tags = [["trip", "camping"], []]
        type = "list-of"
        outcome = { kind = "variant", tag = "done", value = { at = { kind = "timestamp", value = "2026-11-02T07:00:00Z" } } }
        "#,
    );
    // `due` is encouraged: TypeCheck warns, and decoding reads its absence.
    assert_eq!(
        ChecklistDeclaration::declaration()
            .check(&checklist.payload)
            .into_iter()
            .map(|issue| path_text(&issue.path))
            .collect::<Vec<_>>(),
        ["due", "outcome(done).note"]
    );
    assert_eq!(
        checklist.decode::<Checklist>().unwrap(),
        Checklist {
            steps: vec![
                Step {
                    label: "socks".into(),
                    done: true,
                    duration: None
                },
                Step {
                    label: "tent".into(),
                    done: false,
                    duration: Some(SignedDuration::from_mins(20))
                },
            ],
            tags: vec![vec!["trip".into(), "camping".into()], vec![]],
            r#type: Mark::ListOf,
            outcome: Some(Outcome::Finished {
                at: "2026-11-02T07:00:00Z".parse().unwrap(),
                note: None,
            }),
            due: None,
        }
    );
}

#[test]
fn every_tag_decodes_and_a_unit_variant_is_an_empty_tag() {
    let cases = [
        (
            r#"{ kind = "variant", tag = "deferred", value = { horizon = 2 } }"#,
            Decision::Deferred { horizon: 2 },
        ),
        (
            r#"{ kind = "variant", tag = "waiting", value = { horizon = 1 } }"#,
            Decision::Waiting { horizon: 1 },
        ),
        (
            r#"{ kind = "variant", tag = "resumed", value = {} }"#,
            Decision::Resumed {},
        ),
        (
            r#"{ kind = "variant", tag = "sufficient", value = {} }"#,
            Decision::Sufficient {},
        ),
        (
            r#"{ kind = "variant", tag = "abandoned", value = {} }"#,
            Decision::Abandoned {},
        ),
    ];
    for (value, expected) in cases {
        let stance = node("Stance", &format!("decision = {value}"));
        assert_eq!(stance.decode::<Stance>().unwrap().decision, expected);
    }
    let skipped = node(
        "Checklist",
        r#"
        steps = []
        tags = []
        type = "plain_text"
        outcome = { kind = "variant", tag = "skipped", value = {} }
        "#,
    );
    assert_eq!(
        skipped.decode::<Checklist>().unwrap().outcome,
        Some(Outcome::Skipped)
    );
}

#[test]
fn decoding_ignores_fields_the_decode_type_does_not_name() {
    // `at` and `reason` are free under PLANNING §3.4: a user may change them,
    // so the decode type must not read them, even where they no longer fit
    // the canonical declaration.
    let stance = node(
        "Stance",
        r#"
        decision = { kind = "variant", tag = "deferred", value = { horizon = 3, why = "rain" } }
        at = "yesterday"
        reason = 12
        mood = "fine"
        "#,
    );
    assert_eq!(
        stance.decode::<Stance>().unwrap(),
        Stance {
            decision: Decision::Deferred { horizon: 3 }
        }
    );
    let step = Step::from_payload(&Payload::Record(body(
        r#"label = "socks"
done = { kind = "boolean", value = true }
colour = "red""#,
    )))
    .unwrap();
    assert_eq!(step.label, "socks");
}

#[test]
fn decoding_reads_bodies_only() {
    // The type name only serves the error message.
    let habit = node("Something else", "quota = 1\nper = \"week\"");
    assert_eq!(
        habit.decode::<Habit>().unwrap(),
        Habit {
            quota: 1,
            per: Per::Week
        }
    );
    let error = node("Something else", "per = \"week\"")
        .decode::<Habit>()
        .unwrap_err();
    assert_eq!(error.ty_name, "Something else");
}

type Read = fn(&Body) -> Result<(), DecodeError>;

#[test]
fn kinds_are_strict() {
    let wrong = |expected, found| DecodeProblem::WrongKind { expected, found };
    let problem = |toml: &str, decode: Read| decode(&body(toml)).unwrap_err().problem;
    fn read<T: FromPayload>(body: &Body) -> Result<(), DecodeError> {
        crate::decode::field::<T>(body, "value").map(drop)
    }
    let cases: [(&str, Read, DecodeProblem); 9] = [
        (
            r#"value = { kind = "timestamp", value = "2026-11-02T07:00:00Z" }"#,
            read::<String>,
            wrong(PayloadKind::Text, PayloadKind::Timestamp),
        ),
        (
            r#"value = "2026-11-02T07:00:00Z""#,
            read::<Timestamp>,
            wrong(PayloadKind::Timestamp, PayloadKind::Text),
        ),
        (
            r#"value = "2h""#,
            read::<SignedDuration>,
            wrong(PayloadKind::Duration, PayloadKind::Text),
        ),
        (
            r#"value = "2026-11-02""#,
            read::<Date>,
            wrong(PayloadKind::Date, PayloadKind::Text),
        ),
        (
            r#"value = { kind = "timestamp", value = "2026-11-02T07:00:00Z" }"#,
            read::<OffsetTimestamp>,
            wrong(PayloadKind::OffsetTimestamp, PayloadKind::Timestamp),
        ),
        (
            r#"value = "3""#,
            read::<i64>,
            wrong(PayloadKind::Integer, PayloadKind::Text),
        ),
        (
            r#"value = 1"#,
            read::<Per>,
            wrong(PayloadKind::Text, PayloadKind::Integer),
        ),
        (
            r#"value = { kind = "record", value = { horizon = 1 } }"#,
            read::<Decision>,
            wrong(PayloadKind::Variant, PayloadKind::Record),
        ),
        (
            r#"value = { kind = "variant", tag = "socks", value = {} }"#,
            read::<Step>,
            wrong(PayloadKind::Record, PayloadKind::Variant),
        ),
    ];
    for (toml, decode, expected) in cases {
        assert_eq!(problem(toml, decode), expected, "{toml}");
    }
    assert_eq!(
        problem(r#"value = "true""#, read::<bool>),
        wrong(PayloadKind::Boolean, PayloadKind::Text)
    );
    assert_eq!(
        problem(r#"value = "a""#, read::<Vec<String>>),
        wrong(PayloadKind::List, PayloadKind::Text)
    );
}

#[test]
fn an_optional_field_is_absent_or_valid() {
    let none = body("other = 1");
    assert_eq!(optional_field::<i64>(&none, "value"), Ok(None));
    let present = body("value = 2");
    assert_eq!(optional_field::<i64>(&present, "value"), Ok(Some(2)));
    let wrong = body(r#"value = "2""#);
    assert_eq!(
        optional_field::<i64>(&wrong, "value").unwrap_err(),
        DecodeError {
            path: vec![PathPart::Field("value".into())],
            problem: DecodeProblem::WrongKind {
                expected: PayloadKind::Integer,
                found: PayloadKind::Text
            },
        }
    );
}

#[test]
fn errors_name_the_path_under_tags_indexes_and_fields() {
    let field = |name: &str| PathPart::Field(name.into());
    let tag = |name: &str| PathPart::Tag { tag: name.into() };
    let cases = [
        (
            "Stance",
            "at = 1",
            vec![field("decision")],
            DecodeProblem::Missing,
            "decision",
        ),
        (
            "Stance",
            r#"decision = { kind = "variant", tag = "committed", value = { horizon = 2 } }"#,
            vec![field("decision"), tag("committed"), field("cost")],
            DecodeProblem::Missing,
            "decision(committed).cost",
        ),
        (
            "Stance",
            r#"decision = { kind = "variant", tag = "deferred", value = { horizon = "2" } }"#,
            vec![field("decision"), tag("deferred"), field("horizon")],
            DecodeProblem::WrongKind {
                expected: PayloadKind::Integer,
                found: PayloadKind::Text,
            },
            "decision(deferred).horizon",
        ),
        (
            "Stance",
            r#"decision = { kind = "variant", tag = "postponed", value = { cost = 1 } }"#,
            vec![field("decision")],
            DecodeProblem::UnknownTag {
                tag: "postponed".into(),
            },
            "decision",
        ),
        (
            "Habit",
            "quota = 3\nper = \"month\"",
            vec![field("per")],
            DecodeProblem::NotAMember {
                value: "month".into(),
            },
            "per",
        ),
        (
            "Checklist",
            r#"
            type = "plain_text"
            tags = [["a"], ["b", 3]]
            steps = []
            "#,
            vec![field("tags"), PathPart::Index(1), PathPart::Index(1)],
            DecodeProblem::WrongKind {
                expected: PayloadKind::Text,
                found: PayloadKind::Integer,
            },
            "tags[1][1]",
        ),
        (
            "Checklist",
            r#"
            type = "plain_text"
            tags = []
            steps = [{ kind = "record", value = { label = "a", done = { kind = "boolean", value = true } } }, { kind = "record", value = { label = "b" } }]
            "#,
            vec![field("steps"), PathPart::Index(1), field("done")],
            DecodeProblem::Missing,
            "steps[1].done",
        ),
        (
            "Checklist",
            r#"
            type = "plain_text"
            tags = []
            steps = []
            outcome = { kind = "variant", tag = "done", value = { at = "noon" } }
            "#,
            vec![field("outcome"), tag("done"), field("at")],
            DecodeProblem::WrongKind {
                expected: PayloadKind::Timestamp,
                found: PayloadKind::Text,
            },
            "outcome(done).at",
        ),
    ];
    for (ty_name, payload, path, problem, text) in cases {
        let node = node(ty_name, payload);
        let error = match ty_name {
        "Stance" => node.decode::<Stance>().map(drop),
        "Habit" => node.decode::<Habit>().map(drop),
        _ => node.decode::<Checklist>().map(drop),
        }
        .unwrap_err();
        assert_eq!(
            error,
            PayloadError {
                node: node.id,
                ty_name: ty_name.into(),
                error: DecodeError { path, problem },
            }
        );
        assert_eq!(path_text(&error.error.path), text);
    }
}

#[test]
fn errors_read_as_sentences() {
    let message = |ty_name: &str, payload: &str| {
        let node = node(ty_name, payload);
        match ty_name {
        "Stance" => node.decode::<Stance>().map(drop),
        _ => node.decode::<Habit>().map(drop),
        }
        .unwrap_err()
        .to_string()
    };
    assert_eq!(
        message(
            "Stance",
            r#"decision = { kind = "variant", tag = "committed", value = {} }"#
        ),
        r#"cannot decode "Stance" node 7: field decision(committed).cost is missing"#
    );
    assert_eq!(
        message(
            "Stance",
            r#"decision = { kind = "variant", tag = "later", value = {} }"#
        ),
        r#"cannot decode "Stance" node 7: field decision has tag "later", which the decode type does not know"#
    );
    assert_eq!(
        message("Habit", "quota = 1\nper = \"month\""),
        r#"cannot decode "Habit" node 7: field per is "month", which the decode type does not list"#
    );
    assert_eq!(
        message("Habit", "quota = \"1\"\nper = \"day\""),
        r#"cannot decode "Habit" node 7: field quota should be integer, found text"#
    );
    assert_eq!(
        Per::from_payload(&Payload::Integer(1))
            .unwrap_err()
            .to_string(),
        "the value should be text, found integer"
    );
}

#[test]
fn decoded_values_survive_the_payload_codecs() {
    let payload = body(
        r#"
        steps = [{ kind = "record", value = { label = "a", done = { kind = "boolean", value = true }, took = { kind = "duration", value = "90m" } } }]
        tags = [["x"]]
        type = "offset_timestamp"
        outcome = { kind = "variant", tag = "done", value = { at = { kind = "timestamp", value = "2026-11-02T07:00:00Z" }, note = "late" } }
        due = { kind = "date", value = "2026-11-06" }
        "#,
    );
    let decoded = Checklist::from_body(&payload).unwrap();
    let json: Body = serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();
    let toml: Body = toml::from_str(&toml::to_string(&payload).unwrap()).unwrap();
    assert_eq!(Checklist::from_body(&json).unwrap(), decoded);
    assert_eq!(Checklist::from_body(&toml).unwrap(), decoded);
    assert_eq!(
        decoded.steps[0].duration,
        Some(SignedDuration::from_mins(90))
    );
    assert_eq!(decoded.r#type, Mark::OffsetTimestamp);
    assert_eq!(decoded.due, Some(Date::constant(2026, 11, 6)));
}
