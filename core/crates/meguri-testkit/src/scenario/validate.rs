//! What `conformance/schema/scenario.schema.json` requires beyond the types the
//! model reads: required draft-op arguments, lengths, unique items, patterns
//! and the order of handoffs (dsl.md D4-D9). T0 loads every scenario through
//! these checks, so a schema violation is `invalid` there too, not only in
//! `tools/mg goldens-check`. Change both together; `tests/scenario_format.rs`
//! reads the schema's required draft arguments and checks this table.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::FormatError;
use super::model::{DraftArgs, LogicalRef, ObservationKind, RefKind, Scenario, Side, Step};

/// A draft-op argument's JSON shape (scenario.schema.json `$defs/step`).
#[derive(Debug, Clone, Copy)]
enum Arg {
    Str,
    Int,
    Array,
    Strings,
    NoteRef,
    NoteRefs,
    CardRefs,
    Fields,
    OneOf(&'static [&'static str]),
}

/// One argument of a draft op: (name, required, shape).
type ArgSpec = (&'static str, bool, Arg);

/// (op, its arguments): the schema's draft-op arguments. Other keys are
/// allowed: the owning area fixes the full shape (D7.2).
const DRAFT_ARGS: [(&str, &[ArgSpec]); 14] = [
    (
        "edit_note",
        &[
            ("note", true, Arg::NoteRef),
            ("fields", false, Arg::Fields),
            ("tags", false, Arg::Strings),
        ],
    ),
    ("delete_notes", &[("notes", true, Arg::NoteRefs)]),
    ("bury", &[("cards", true, Arg::CardRefs)]),
    ("suspend", &[("cards", true, Arg::CardRefs)]),
    ("forget", &[("cards", true, Arg::CardRefs)]),
    ("set_due", &[("cards", true, Arg::CardRefs), ("days", true, Arg::Str)]),
    (
        "reposition",
        &[("cards", true, Arg::CardRefs), ("start", true, Arg::Int)],
    ),
    ("update_preset", &[("name", true, Arg::Str)]),
    (
        "build_filtered",
        &[("name", true, Arg::Str), ("search", true, Arg::Str)],
    ),
    ("import", &[("fixture", true, Arg::Str)]),
    (
        "export",
        &[
            ("format", true, Arg::OneOf(&["apkg", "colpkg"])),
            ("name", true, Arg::Str),
        ],
    ),
    ("check_db", &[]),
    ("db_exec", &[("sql", true, Arg::Str), ("params", false, Arg::Array)]),
    (
        "sync",
        &[
            ("kind", false, Arg::OneOf(&["normal", "full_upload", "full_download"])),
            ("client", false, Arg::Str),
        ],
    ),
];

pub(super) fn scenario(s: &Scenario) -> Result<(), FormatError> {
    let mut tags = BTreeSet::new();
    if let Some(tag) = s.tags.iter().find(|t| !tags.insert(t.as_str())) {
        return Err(FormatError::new(format!("tag {tag:?} appears twice")));
    }
    if s.clock.tz.is_empty() {
        return Err(FormatError::new("clock.tz must name an IANA time zone"));
    }
    for pattern in &s.compare.set {
        if !pattern.starts_with('/') {
            return Err(FormatError::new(format!(
                "compare.set entry {pattern:?} is not a pointer pattern (/steps/*/...)"
            )));
        }
    }
    let empty = |what: &str| FormatError::new(format!("{what} must not be empty"));
    for preset in s.setup.presets.iter().flatten() {
        if preset.name.is_empty() {
            return Err(empty("a preset name"));
        }
    }
    for deck in s.setup.decks.iter().flatten() {
        if deck.name().is_empty() {
            return Err(empty("a deck name"));
        }
    }
    for notetype in s.setup.notetypes.iter().flatten() {
        if notetype.name.is_empty() {
            return Err(empty("a note type name"));
        }
        if notetype.fields.is_empty() {
            return Err(empty(&format!("note type {:?}'s field list", notetype.name)));
        }
        if notetype.templates.is_empty() {
            return Err(empty(&format!("note type {:?}'s template list", notetype.name)));
        }
    }
    let mut side: Option<Side> = None;
    for (index, step) in s.steps.iter().enumerate() {
        let at = |e: FormatError| FormatError::new(format!("step {index} ({}): {e}", step.name()));
        if let Some(args) = step.draft_args() {
            draft(step.name(), args).map_err(at)?;
        }
        if let Step::Handoff(to) = step {
            if side == Some(*to) {
                return Err(at(FormatError::new(format!(
                    "the collection is already on the {} side; handoffs alternate",
                    to.as_str()
                ))));
            }
            side = Some(*to);
        }
    }
    Ok(())
}

fn draft(op: &str, args: &DraftArgs) -> Result<(), FormatError> {
    let Some((_, spec)) = DRAFT_ARGS.iter().find(|(name, _)| *name == op) else {
        return Ok(());
    };
    for (key, required, shape) in spec.iter() {
        match args.0.get(*key) {
            None if *required => return Err(FormatError::new(format!("{op} needs {key}"))),
            None => {}
            Some(value) => {
                check(*shape, value).map_err(|why| FormatError::new(format!("{op}.{key}: {why}")))?;
            }
        }
    }
    Ok(())
}

fn reference(value: &Value, kind: RefKind) -> Result<(), String> {
    let r: LogicalRef = value
        .as_str()
        .ok_or_else(|| format!("{value} is not a reference"))?
        .parse()
        .map_err(|e: FormatError| e.to_string())?;
    if r.kind != kind {
        return Err(format!(
            "{r} is not a {} reference",
            if kind == RefKind::Card { "card" } else { "note" }
        ));
    }
    Ok(())
}

fn references(value: &Value, kind: RefKind) -> Result<(), String> {
    let items = value.as_array().ok_or("a list of references")?;
    if items.is_empty() {
        return Err("at least one reference".to_owned());
    }
    items.iter().try_for_each(|v| reference(v, kind))
}

fn check(shape: Arg, value: &Value) -> Result<(), String> {
    let ok = |b: bool, what: &str| if b { Ok(()) } else { Err(format!("must be {what}")) };
    match shape {
        Arg::Str => ok(value.is_string(), "a string"),
        Arg::Int => ok(value.is_i64() || value.is_u64(), "an integer"),
        Arg::Array => ok(value.is_array(), "a list"),
        Arg::Strings => ok(
            value.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
            "a list of strings",
        ),
        Arg::NoteRef => reference(value, RefKind::Note),
        Arg::NoteRefs => references(value, RefKind::Note),
        Arg::CardRefs => references(value, RefKind::Card),
        Arg::Fields => ok(
            value.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                || value.as_object().is_some_and(|m| m.values().all(Value::is_string)),
            "a list of strings or a mapping of field names to strings",
        ),
        Arg::OneOf(values) => ok(
            value.as_str().is_some_and(|v| values.contains(&v)),
            &format!("one of {}", values.join(", ")),
        ),
    }
}

/// The schema's rules for the mapping form of an observation request.
pub(super) fn observation_mapping(kind: ObservationKind, map: &Map<String, Value>) -> Result<(), FormatError> {
    let name = kind.name();
    let bad = |why: &str| Err(FormatError::new(format!("observation {name}: {why}")));
    let uint = |v: &Value| v.is_u64();
    let only = |keys: &[&str]| map.keys().all(|k| keys.contains(&k.as_str()));
    match kind {
        ObservationKind::FuzzTable => {
            let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
            if keys != BTreeSet::from(["card", "intervals", "pairs"]) {
                return bad("takes {card, pairs, intervals}");
            }
            if reference(&map["card"], RefKind::Card).is_err() {
                return bad("card is a #cN reference");
            }
            let pairs_ok = map["pairs"].as_array().is_some_and(|pairs| {
                pairs
                    .iter()
                    .all(|p| p.as_array().is_some_and(|p| p.len() == 2 && p.iter().all(uint)))
            });
            if !pairs_ok {
                return bad("pairs is a list of [card_id, reps] pairs of non-negative integers");
            }
            if !map["intervals"]
                .as_array()
                .is_some_and(|i| !i.is_empty() && i.iter().all(uint))
            {
                return bad("intervals is a non-empty list of non-negative integers");
            }
        }
        ObservationKind::NetProbe => {
            if !only(&["host", "port", "timeout_ms"])
                || !map.get("host").is_some_and(Value::is_string)
                || !map.get("port").is_some_and(|p| p.is_i64() || p.is_u64())
                || map.get("timeout_ms").is_some_and(|t| !uint(t))
            {
                return bad("takes {host, port, timeout_ms?}");
            }
        }
        ObservationKind::Search => {
            if !map.get("query").is_some_and(Value::is_string) {
                return bad("needs a query string");
            }
            if map
                .get("kind")
                .is_some_and(|k| !matches!(k.as_str(), Some("cards" | "notes")))
            {
                return bad("kind is cards or notes");
            }
        }
        ObservationKind::Queue => {
            if map.get("deck").is_some_and(|d| !d.is_string()) {
                return bad("deck is a deck name");
            }
        }
        ObservationKind::DbDump
            if map
                .get("tables")
                .is_some_and(|t| !t.as_array().is_some_and(|a| a.iter().all(Value::is_string))) =>
        {
            return bad("tables is a list of table names");
        }
        _ => {}
    }
    Ok(())
}

/// The draft ops and their required arguments, for the schema cross-check.
pub fn draft_requirements() -> Vec<(&'static str, Vec<&'static str>)> {
    DRAFT_ARGS
        .iter()
        .map(|(op, spec)| (*op, spec.iter().filter(|(_, r, _)| *r).map(|(k, _, _)| *k).collect()))
        .collect()
}
