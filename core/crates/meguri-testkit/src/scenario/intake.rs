//! Recognition of prepared maintenance/package vocabulary. No execution support.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::{FormatError, LogicalRef, ObservationArgs, ObservationKind, RefKind, Scenario, Step};

fn invalid(why: &str) -> FormatError {
    FormatError::new(why)
}

pub(super) fn session_name(name: &str) -> bool {
    name != "collection.anki2"
        && name.ends_with(".anki2")
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

fn keys(map: &Map<String, Value>, required: &[&str], optional: &[&str]) -> Result<(), FormatError> {
    if required.iter().all(|key| map.contains_key(*key))
        && map
            .keys()
            .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()))
    {
        Ok(())
    } else {
        Err(invalid("missing or unknown argument"))
    }
}

fn text(value: Option<&Value>, nonempty: bool) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| !nonempty || !s.is_empty())
}

fn names(value: Option<&Value>, allowed: Option<&[&str]>, distinct: bool) -> bool {
    value.and_then(Value::as_array).is_some_and(|items| {
        let mut seen = BTreeSet::new();
        !items.is_empty()
            && items.iter().all(|item| {
                item.as_str().is_some_and(|name| {
                    !name.is_empty()
                        && allowed.is_none_or(|allowed| allowed.contains(&name))
                        && (!distinct || seen.insert(name))
                })
            })
    })
}

fn refs(value: &Value, kind: Option<RefKind>, distinct: bool) -> bool {
    value.as_array().is_some_and(|items| {
        let mut seen = BTreeSet::new();
        !items.is_empty()
            && items.iter().all(|item| {
                item.as_str()
                    .and_then(|s| s.parse::<LogicalRef>().ok())
                    .is_some_and(|r| {
                        kind.is_none_or(|kind| r.kind == kind)
                            && matches!(r.kind, RefKind::Card | RefKind::Note)
                            && (!distinct || seen.insert(r))
                    })
            })
    })
}

pub(super) fn step(step: &Step) -> Result<(), FormatError> {
    let valid = match step {
        Step::RemoveNotetype(a) => !a.notetype.is_empty(),
        Step::RestoreNotetype(a) => !a.notetype.is_empty(),
        Step::ChangeNotetypeInfo(a) => !a.notetype.is_empty() && !a.notes.is_empty(),
        Step::ChangeNotetype(a) => {
            let mut seen = BTreeSet::new();
            a.templates
                .as_ref()
                .is_none_or(|map| map.iter().flatten().all(|ord| seen.insert(*ord)))
        }
        Step::Schema11Copy(a) => session_name(&a.name),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("invalid maintenance arguments"))
    }
}

/// Only prerequisites knowable without running the collection. Successful info,
/// schema stamps, map lengths/ordinals and Browse mode are execution-time rules.
pub(super) fn chronology(s: &Scenario) -> Result<(), FormatError> {
    let mut media = false;
    let mut selections = BTreeSet::new();
    let mut scans = BTreeSet::new();
    let mut browse_modes = BTreeMap::new();
    if !s.starts_with_open() {
        if s.has_actors() {
            for actor in s.actors() {
                browse_modes.insert(Some(actor), false);
            }
            browse_modes.insert(
                s.actors().first(),
                s.setup
                    .config
                    .as_ref()
                    .is_some_and(|c| c.get("browserTableShowNotesMode") == Some(&Value::Bool(true))),
            );
        } else {
            browse_modes.insert(
                None,
                s.setup
                    .config
                    .as_ref()
                    .is_some_and(|c| c.get("browserTableShowNotesMode") == Some(&Value::Bool(true))),
            );
        }
    }
    for ((index, step), actor) in s.steps.iter().enumerate().zip(s.step_actors()) {
        let at = |why| FormatError::new(format!("step {index} ({}): {why}", step.name()));
        match step {
            Step::CheckMedia(_) => media = true,
            Step::DeleteUnusedMedia(_) | Step::TagMissingMedia(_) if !media => {
                return Err(at("requires an earlier uninvalidated check_media"));
            }
            Step::Observe(_) | Step::Advance(_) | Step::SetClock(_) | Step::TagMissingMedia(_) => {}
            _ => media = false,
        }
        match step {
            Step::Handoff(_) => {
                selections.clear();
                scans.clear();
                browse_modes.clear();
            }
            Step::ChangeNotetypeInfo(_) => {
                selections.insert(actor);
            }
            Step::ChangeNotetype(_) if !selections.contains(&actor) => {
                return Err(at("requires earlier change_notetype_info in this segment"));
            }
            Step::EmptyCards(_) => {
                scans.insert(actor);
            }
            Step::DeleteEmptyCards(_) if !scans.contains(&actor) => {
                return Err(at("requires earlier empty_cards in this segment"));
            }
            Step::SetConfig(a) if a.key == "browserTableShowNotesMode" => {
                browse_modes.insert(actor, a.value == Value::Bool(true));
            }
            Step::BrowserConfigSet(a) if a.key == "browserTableShowNotesMode" => {
                if let Some(notes) = a.value.as_bool() {
                    browse_modes.insert(actor, notes);
                } else {
                    browse_modes.remove(&actor);
                }
            }
            Step::RemoveConfig(a) if a.key == "browserTableShowNotesMode" => {
                browse_modes.insert(actor, false);
            }
            Step::Open(_) | Step::Reopen(_) | Step::Import(_) | Step::DbExec(_) | Step::Undo(_) | Step::Redo(_) => {
                browse_modes.remove(&actor);
            }
            Step::Observe(requests) => {
                // SQL, imports, reopen and handoffs may leave the mode unknown
                // until an explicit config step; the adapter then checks it.
                let mode = browse_modes.get(&actor).copied();
                if let Some(notes) = mode {
                    for request in requests.iter().filter(|r| r.kind == ObservationKind::BrowserRows) {
                        if let ObservationArgs::Mapping(map) = &request.args
                            && !refs(
                                &map["rows"],
                                Some(if notes { RefKind::Note } else { RefKind::Card }),
                                true,
                            )
                        {
                            return Err(at("browser_rows references disagree with Browse mode"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn apkg(op: &str, map: &Map<String, Value>) -> Result<(), FormatError> {
    if op == "import" {
        keys(
            map,
            &[
                "format",
                "merge_notetypes",
                "update_notes",
                "update_notetypes",
                "with_scheduling",
                "with_deck_configs",
            ],
            &["fixture", "name"],
        )?;
        let source: Map<String, Value> = map
            .iter()
            .filter(|(k, _)| matches!(k.as_str(), "name" | "fixture"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        super::PackageListingRequest::from_mapping(&source)?;
        if !source
            .values()
            .all(|v| v.as_str().is_some_and(|s| s.ends_with(".apkg")))
            || ["merge_notetypes", "with_scheduling", "with_deck_configs"]
                .iter()
                .any(|k| !map[*k].is_boolean())
            || ["update_notes", "update_notetypes"].iter().any(|k| {
                !map[*k]
                    .as_str()
                    .is_some_and(|v| ["if_newer", "always", "never"].contains(&v))
            })
        {
            return Err(invalid("invalid deck package import source or options"));
        }
    } else {
        keys(map, &["format", "name", "limit", "scheduling", "presets", "media"], &[])?;
        if !map["name"]
            .as_str()
            .is_some_and(|s| super::package::package_name(s, &[".apkg"]))
            || ["scheduling", "presets", "media"].iter().any(|k| !map[*k].is_boolean())
        {
            return Err(invalid("invalid deck package export name or flags"));
        }
        let limit = &map["limit"];
        let valid = limit.as_str() == Some("collection")
            || limit.as_object().is_some_and(|limit| {
                limit.len() == 1
                    && limit.iter().all(|(key, v)| match key.as_str() {
                        "deck" => text(Some(v), true),
                        "notes" => refs(v, Some(RefKind::Note), false),
                        "cards" => refs(v, Some(RefKind::Card), false),
                        _ => false,
                    })
            });
        if !valid {
            return Err(invalid("invalid deck package export limit"));
        }
    }
    Ok(())
}

pub(super) fn today_stats(items: &[Value]) -> Result<(), FormatError> {
    if items.is_empty()
        || !items.iter().all(|scope| {
            scope.as_str() == Some("collection")
                || scope.as_object().is_some_and(|scope| {
                    scope.len() == 1
                        && scope.iter().all(|(key, v)| match key.as_str() {
                            "deck" => text(Some(v), true),
                            "search" => text(Some(v), false),
                            _ => false,
                        })
                })
        })
    {
        return Err(invalid("today_stats needs collection, deck or search scopes"));
    }
    Ok(())
}

const BROWSER_COLUMNS: &[&str] = &[
    "noteFld",
    "template",
    "deck",
    "note",
    "noteTags",
    "noteCrt",
    "noteMod",
    "cardMod",
    "cardDue",
    "cardIvl",
    "cardEase",
    "cardReps",
    "cardLapses",
    "question",
    "answer",
    "stability",
    "difficulty",
    "retrievability",
    "originalPosition",
];
const FILE_COLUMNS: &[&str] = &[
    "crt", "mod", "scm", "ver", "dty", "usn", "ls", "conf", "models", "decks", "dconf", "tags",
];

/// Returns false for kinds handled by the existing validators.
pub(super) fn observation(kind: ObservationKind, map: &Map<String, Value>) -> Result<bool, FormatError> {
    let valid = match kind {
        ObservationKind::AddDefaults => {
            super::AddDefaultsRequest::from_mapping(map)?;
            true
        }
        ObservationKind::NotetypeLastDeck => {
            super::NotetypeLastDeckRequest::from_mapping(map)?;
            true
        }
        ObservationKind::TagComplete => {
            super::TagCompleteRequest::from_mapping(map)?;
            true
        }
        ObservationKind::BrowserRows => {
            keys(map, &["columns", "rows"], &[])?;
            names(map.get("columns"), Some(BROWSER_COLUMNS), true)
                && refs(&map["rows"], None, true)
                && map["rows"].as_array().is_some_and(|rows| {
                    let kinds: BTreeSet<_> = rows
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|r| r.parse::<LogicalRef>().ok())
                        .map(|r| r.kind)
                        .collect();
                    kinds.len() == 1
                })
        }
        ObservationKind::NotetypeTemplates => {
            keys(map, &["notetypes"], &[])?;
            names(map.get("notetypes"), None, false)
        }
        ObservationKind::StudiedTodayFormat => {
            keys(map, &["inputs"], &[])?;
            map["inputs"].as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.iter().all(|item| {
                        item.as_array().is_some_and(|pair| {
                            pair.len() == 2
                                && pair[0].as_u64().is_some_and(|n| n <= u64::from(u32::MAX))
                                && pair[1]
                                    .as_i64()
                                    .is_some_and(|n| (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&n))
                        })
                    })
            })
        }
        ObservationKind::FileCol | ObservationKind::FileObjects | ObservationKind::FileQuery => {
            let extra = match kind {
                ObservationKind::FileCol => "columns",
                ObservationKind::FileQuery => "sql",
                _ => "",
            };
            let required = if extra.is_empty() {
                vec!["name"]
            } else {
                vec!["name", extra]
            };
            keys(map, &required, &[])?;
            map["name"].as_str().is_some_and(session_name)
                && match kind {
                    ObservationKind::FileCol => names(map.get("columns"), Some(FILE_COLUMNS), true),
                    ObservationKind::FileQuery => text(map.get("sql"), true),
                    _ => true,
                }
        }
        ObservationKind::PackageMedia | ObservationKind::PackageDbDump => {
            let mut source = map.clone();
            if kind == ObservationKind::PackageDbDump {
                let mut dump = Map::new();
                for key in ["tables", "raw"] {
                    if let Some(value) = source.remove(key) {
                        dump.insert(key.to_owned(), value);
                    }
                }
                super::validate::observation_mapping(ObservationKind::DbDump, &dump)?;
            }
            super::PackageListingRequest::from_mapping(&source)?;
            true
        }
        _ => return Ok(false),
    };
    if valid {
        Ok(true)
    } else {
        Err(FormatError::new(format!("invalid {} request", kind.name())))
    }
}
