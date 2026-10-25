//! What `conformance/schema/scenario.schema.json` requires beyond the types the
//! model reads: required draft-op arguments, lengths, unique items, patterns,
//! the order of handoffs (dsl.md D4-D9) and the actor rules the schema cannot
//! express (sync-ops.md S1, S2.3, S4.2, S13). T0 loads every scenario through
//! these checks, so a schema violation is `invalid` there too, not only in
//! `tools/mg goldens-check`. Change both together; `tests/scenario_format.rs`
//! reads the schema's required draft arguments and checks this table.

use std::collections::BTreeSet;

use meguri_search::config::{Key, Shape};
use serde_json::{Map, Value};

use super::FormatError;
use super::model::{
    ActorName, CardsOrNotes, ClozeRequest, DUMP_TABLES, DraftArgs, FieldsCheckRequest, FsrsNextRequest,
    IgnoredBeforeCountRequest, LegacyUpdate, LogicalRef, ObservationKind, RefKind, ReviewSession, SaveDeckOptionsArgs,
    Scenario, SearchRequest, Side, Step, SyncKind, TemplateRequest, TextRequest, canonical_search_texts,
};

/// A draft-op argument's JSON shape (scenario.schema.json `$defs/step`).
#[derive(Debug, Clone, Copy)]
enum Arg {
    Str,
    Int,
    CardRefs,
    OneOf(&'static [&'static str]),
}

/// One argument of a draft op: (name, required, shape).
type ArgSpec = (&'static str, bool, Arg);

/// (op, its arguments): the schema's draft-op arguments. Other keys are
/// allowed: the owning area fixes the full shape (D7.2).
const DRAFT_ARGS: [(&str, &[ArgSpec]); 5] = [
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
        notetype_spec(notetype)?;
    }
    if let Some(files) = &s.setup.media_files {
        media_files(files, false)?;
    }
    actors(s)?;
    opens(s)?;
    review_sessions(s)?;
    let mut side: Option<Side> = None;
    for (index, step) in s.steps.iter().enumerate() {
        let at = |e: FormatError| FormatError::new(format!("step {index} ({}): {e}", step.name()));
        if let Some(args) = step.draft_args() {
            draft(step.name(), args).map_err(at)?;
        }
        if let Step::LegacyUpdate(update) = step {
            legacy_update(update).map_err(at)?;
        }
        if let Step::SaveDeckOptions(args) = step {
            save_deck_options(args).map_err(at)?;
        }
        if let Step::SetSchedulingPrefs(a) = step {
            if a.rollover.is_none() && a.learn_ahead_minutes.is_none() && a.load_balancer.is_none() {
                return Err(at(FormatError::new("set_scheduling_prefs needs a setting")));
            }
            if a.rollover.is_some_and(|v| !(0..=23).contains(&v))
                || a.learn_ahead_minutes.is_some_and(|v| !(0..=999).contains(&v))
            {
                return Err(at(FormatError::new("scheduling preference out of range")));
            }
        }
        notetype_op(step).map_err(at)?;
        record_op(step).map_err(at)?;
        deck_op(step).map_err(at)?;
        tag_op(step).map_err(at)?;
        sched_op(step).map_err(at)?;
        browser_config_op(step).map_err(at)?;
        match step {
            Step::MediaOutside(files) => {
                if files.is_empty() {
                    return Err(at(empty("media_outside")));
                }
                media_files(files, true).map_err(at)?;
            }
            Step::AddMedia(file) => media_file(file, true, false).map_err(at)?,
            Step::TrashMedia(args) if args.names.is_empty() || args.names.iter().any(|n| !media_component(n)) => {
                return Err(at(FormatError::new("trash_media needs non-empty file names")));
            }
            _ => {}
        }
        if let Step::Observe(requests) = step {
            once_per_step(requests).map_err(at)?;
        }
        if let Step::Export(args) = step {
            legacy_export(args).map_err(at)?;
        }
        // float-determinism.md F5.1: at most one fsrs_next per observe step.
        if let Step::Observe(requests) = step
            && requests.iter().filter(|r| r.kind == ObservationKind::FsrsNext).count() > 1
        {
            return Err(at(FormatError::new(
                "fsrs_next appears at most once in an observe step",
            )));
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

pub(super) fn media_component(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && !name.contains(['/', '\0']) && !matches!(name, "." | "..")
}
fn media_file(file: &super::MediaFileSpec, add: bool, remove: bool) -> Result<(), FormatError> {
    let forms = [
        file.text.is_some(),
        file.fill.is_some(),
        file.dir.is_some(),
        file.remove.is_some(),
    ];
    if forms.iter().filter(|p| **p).count() != 1
        || file.dir == Some(false)
        || file.remove == Some(false)
        || (add && (file.dir.is_some() || file.remove.is_some()))
        || (!remove && file.remove.is_some())
        || (!add && !media_component(&file.name))
        || file.fill.as_ref().is_some_and(|fill| fill.size > 209_715_200)
    {
        return Err(FormatError::new("invalid media file name or content form"));
    }
    Ok(())
}
fn media_files(files: &[super::MediaFileSpec], remove: bool) -> Result<(), FormatError> {
    let mut names = Vec::<String>::new();
    for file in files {
        media_file(file, false, remove)?;
        let name = meguri_text::normalize::nfc(&file.name).into_owned();
        if names
            .iter()
            .any(|other| meguri_text::fold::compare_folded(other, &name).is_eq())
        {
            return Err(FormatError::new(
                "media names must be distinct after NFC and unicase comparison",
            ));
        }
        names.push(name);
    }
    Ok(())
}

/// The actor rules (sync-ops.md S1, S2.1, S2.3, S4.2).
fn actors(s: &Scenario) -> Result<(), FormatError> {
    let Some(actors) = &s.actors else {
        if s.server.is_some() {
            return Err(FormatError::new("server needs actors (sync-ops.md S3.2)"));
        }
        if let Some((index, step)) = s
            .steps
            .iter()
            .enumerate()
            .find(|(_, step)| Step::ACTOR_OPS.contains(&step.name()))
        {
            return Err(FormatError::new(format!(
                "step {index} ({}): only actor scenarios have this op; declare actors (sync-ops.md S1)",
                step.name()
            )));
        }
        return Ok(());
    };
    if actors.is_empty() {
        return Err(FormatError::new("actors must not be empty"));
    }
    let mut seen = BTreeSet::new();
    if let Some(name) = actors.iter().find(|a| !seen.insert(a.as_str())) {
        return Err(FormatError::new(format!("actor {name} appears twice in actors")));
    }
    let mut logged_in: BTreeSet<&ActorName> = BTreeSet::new();
    for ((index, step), by) in s.steps.iter().enumerate().zip(s.step_actors()) {
        let at = |why: String| Err(FormatError::new(format!("step {index} ({}): {why}", step.name())));
        match step {
            Step::Handoff(_) => return at("an actor scenario has no handoff step (sync-ops.md S1)".to_owned()),
            Step::Adopt(_) => return at("an actor scenario has no adopt step (normalized-dump.md N5)".to_owned()),
            Step::Actor(name) if !actors.contains(name) => {
                return at(format!("actor {name} is not declared in actors (sync-ops.md S2.3)"));
            }
            Step::Sync(sync) => {
                let Some(actor) = by else { continue };
                if logged_in.insert(actor) && sync.kind != SyncKind::Login {
                    return at(format!(
                        "{actor}'s first sync step is {}; it must be a login (sync-ops.md S4.2)",
                        sync.kind.as_str()
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// storage-ops.md P1.3: a scenario with an `open` step has no setup, its
/// first step is `open`, and it has no actors (each actor gets a new
/// collection, sync-ops.md S2.2).
fn opens(s: &Scenario) -> Result<(), FormatError> {
    if !s.opens_fixtures() {
        return Ok(());
    }
    if !s.setup.sections().is_empty() {
        return Err(FormatError::new(
            "a scenario with an open step has no setup (storage-ops.md P1.3)",
        ));
    }
    if !matches!(s.steps.first(), Some(Step::Open(_))) {
        return Err(FormatError::new(
            "a scenario with an open step starts with open (storage-ops.md P1.3)",
        ));
    }
    if s.has_actors() {
        return Err(FormatError::new("an actor scenario has no open step"));
    }
    Ok(())
}

/// sync-ops.md S13: each actor (or the sole collection) has its own session.
fn review_sessions(s: &Scenario) -> Result<(), FormatError> {
    let mut open = BTreeSet::new();
    for ((index, step), by) in s.steps.iter().enumerate().zip(s.step_actors()) {
        let why = match step {
            Step::ReviewSession(ReviewSession::Begin) if !open.insert(by) => {
                Some("review_session begin requires no open session")
            }
            Step::ReviewSession(ReviewSession::End) if !open.remove(&by) => {
                Some("review_session end requires an open session")
            }
            Step::Reopen(_) if open.contains(&by) => Some("cannot close a collection with an open review_session"),
            Step::Sync(sync)
                if matches!(sync.kind, SyncKind::FullUpload | SyncKind::FullDownload) && open.contains(&by) =>
            {
                Some("cannot close a collection with an open review_session")
            }
            _ => None,
        };
        if let Some(why) = why {
            return Err(FormatError::new(format!(
                "step {index} ({}): {why} (sync-ops.md S13)",
                step.name()
            )));
        }
    }
    // An open session at EOF is valid.
    Ok(())
}

fn draft(op: &str, args: &DraftArgs) -> Result<(), FormatError> {
    if args.0.get("format").and_then(Value::as_str) == Some("colpkg") {
        match op {
            "import" => return super::CollectionPackageImportArgs::from_mapping(&args.0).map(drop),
            // Preserve the separate legacy-export diagnostic below.
            "export" if !args.0.contains_key("legacy") => {
                return super::CollectionPackageExportArgs::from_mapping(&args.0).map(drop);
            }
            _ => {}
        }
    }
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
        Arg::CardRefs => references(value, RefKind::Card),
        Arg::OneOf(values) => ok(
            value.as_str().is_some_and(|v| values.contains(&v)),
            &format!("one of {}", values.join(", ")),
        ),
    }
}

/// record-ops.md W1, W3, W4 and undo-ops.md V3.2: what the schema requires
/// beyond the types.
fn record_op(step: &Step) -> Result<(), FormatError> {
    match step {
        Step::DbExec(args) if args.sql.is_empty() => Err(FormatError::new("db_exec needs a non-empty sql")),
        Step::DeleteNotes(args) if args.notes.is_empty() => {
            Err(FormatError::new("delete_notes needs at least one note"))
        }
        Step::SetConfig(args) if args.key.is_empty() => Err(FormatError::new("set_config needs a non-empty key")),
        Step::RemoveConfig(args) if args.key.is_empty() => Err(FormatError::new("remove_config needs a non-empty key")),
        _ => Ok(()),
    }
}

/// deck-ops.md K1-K10: what the schema requires beyond the types (decks
/// named by non-empty stored names, at least one in a list) and K9's two
/// different presets.
fn deck_op(step: &Step) -> Result<(), FormatError> {
    let op = step.name();
    let named = |names: &[String]| {
        if names.iter().any(String::is_empty) {
            return Err(FormatError::new(format!("{op}: a deck name must not be empty")));
        }
        Ok(())
    };
    let listed = |decks: &[String]| {
        if decks.is_empty() {
            return Err(FormatError::new(format!("{op} needs at least one deck")));
        }
        named(decks)
    };
    match step {
        Step::RenameDeck(args) => named(std::slice::from_ref(&args.deck)),
        Step::SetCurrentDeck(args) => named(std::slice::from_ref(&args.deck)),
        Step::SetDeckPreset(args) => named(std::slice::from_ref(&args.deck)),
        Step::ReparentDecks(args) => listed(&args.decks),
        Step::DeleteDecks(args) => listed(&args.decks),
        Step::SetDeckCollapsed(args) => listed(&args.decks),
        Step::RemovePreset(args) if args.preset == args.fallback => Err(FormatError::new(
            "remove_preset: preset and fallback must differ (deck-ops.md K9)",
        )),
        _ => Ok(()),
    }
}

/// tag-ops.md G1 to G8: what the schema requires beyond the types.
fn tag_op(step: &Step) -> Result<(), FormatError> {
    let names = |op: &str, names: &[String]| {
        if names.is_empty() {
            Err(FormatError::new(format!("{op} needs at least one tag")))
        } else if names.iter().any(String::is_empty) {
            Err(FormatError::new(format!("{op} needs non-empty tag names")))
        } else {
            Ok(())
        }
    };
    match step {
        Step::AddNoteTags(args) | Step::RemoveNoteTags(args) if args.notes.is_empty() => {
            Err(FormatError::new(format!("{} needs at least one note", step.name())))
        }
        Step::RenameTag(args) if args.from.is_empty() || args.from.contains([' ', '\u{3000}']) => Err(
            FormatError::new("rename_tag needs a non-empty from without U+0020 or U+3000"),
        ),
        Step::ReparentTags(args) => names("reparent_tags", &args.tags),
        Step::SetTagCollapsed(args) => names("set_tag_collapsed", &args.tags),
        Step::SetFlag(args) if args.cards.is_empty() => Err(FormatError::new("set_flag needs at least one card")),
        _ => Ok(()),
    }
}

/// sched-ops.md J1, J5, J9: what the schema requires beyond the types (at
/// least one card or note, each listed once; decks named by non-empty stored
/// names).
fn sched_op(step: &Step) -> Result<(), FormatError> {
    let op = step.name();
    let listed = |refs: Vec<LogicalRef>| {
        if refs.is_empty() {
            return Err(FormatError::new(format!("{op} needs at least one reference")));
        }
        let mut seen = BTreeSet::new();
        match refs.into_iter().find(|r| !seen.insert(*r)) {
            Some(r) => Err(FormatError::new(format!("{op} lists {r} twice (sched-ops.md J1)"))),
            None => Ok(()),
        }
    };
    let cards = |cards: &[super::model::CardRef]| listed(cards.iter().map(|c| c.0).collect());
    let deck = |deck: &str| {
        if deck.is_empty() {
            return Err(FormatError::new(format!("{op}: a deck name must not be empty")));
        }
        Ok(())
    };
    match step {
        Step::Bury(targets) | Step::Suspend(targets) => match targets {
            CardsOrNotes::Cards(refs) => cards(refs),
            CardsOrNotes::Notes(refs) => listed(refs.iter().map(|n| n.0).collect()),
        },
        Step::Unsuspend(args) => cards(&args.cards),
        Step::SetDue(args) => cards(&args.cards),
        Step::Forget(args) => cards(&args.cards),
        Step::GradeNow(args) => cards(&args.cards),
        Step::UnburyDeck(args) => deck(&args.deck),
        Step::EmptyFiltered(args) => deck(&args.deck),
        _ => Ok(()),
    }
}

/// browser-config-ops.md B1: one of the 22 browser config keys (every
/// browser key but `normalize_note_text`) and a value of its shape.
fn browser_config_op(step: &Step) -> Result<(), FormatError> {
    let Step::BrowserConfigSet(args) = step else {
        return Ok(());
    };
    let Some(key) = Key::from_name(&args.key).filter(|key| key.writable()) else {
        return Err(FormatError::new(format!(
            "browser_config_set: {:?} is not one of the 22 browser config keys",
            args.key
        )));
    };
    let fits = match (key.shape(), &args.value) {
        (Shape::Boolean(_), Value::Bool(_)) => true,
        (Shape::Text(_) | Shape::SortColumn, Value::String(_)) => true,
        (Shape::Columns, Value::Array(items)) => items.iter().all(Value::is_string),
        (Shape::Names, Value::Object(members)) => members.values().all(Value::is_string),
        _ => false,
    };
    if fits {
        Ok(())
    } else {
        Err(FormatError::new(format!(
            "browser_config_set: {} takes {}, found {}",
            args.key,
            match key.shape() {
                Shape::Boolean(_) => "a boolean",
                Shape::Text(_) | Shape::SortColumn => "a string",
                Shape::Columns => "an array of strings",
                Shape::Names => "an object of strings",
            },
            args.value
        )))
    }
}

/// model-ops.md L2: exactly one non-empty record name.
fn legacy_update(update: &LegacyUpdate) -> Result<(), FormatError> {
    let names = [&update.notetype, &update.deck, &update.preset];
    let given: Vec<&String> = names.iter().filter_map(|name| name.as_ref()).collect();
    if given.len() != 1 {
        return Err(FormatError::new(
            "legacy_update names exactly one of notetype, deck and preset",
        ));
    }
    if given[0].is_empty() {
        return Err(FormatError::new("legacy_update needs a non-empty name"));
    }
    Ok(())
}

/// deck-options-ops.md E1: non-empty deck and preset names.
fn save_deck_options(args: &SaveDeckOptionsArgs) -> Result<(), FormatError> {
    if args.deck.is_empty() {
        return Err(FormatError::new("save_deck_options needs a non-empty deck name"));
    }
    if args.preset.as_ref().is_some_and(String::is_empty) {
        return Err(FormatError::new("save_deck_options preset is a non-empty name"));
    }
    if let Some(limits) = &args.limits {
        for value in [limits.new, limits.review, limits.new_today, limits.review_today]
            .into_iter()
            .flatten()
            .flatten()
        {
            if !(0..=9999).contains(&value) {
                return Err(FormatError::new("deck limit out of range"));
            }
        }
    }
    if let Some(names) = &args.remove {
        let unique = names.iter().collect::<BTreeSet<_>>();
        if names.is_empty() || unique.len() != names.len() || names.iter().any(String::is_empty) {
            return Err(FormatError::new("remove needs distinct nonempty preset names"));
        }
    }
    Ok(())
}

/// A `setup.notetypes` entry or `add_notetype` (dsl.md D6.4,
/// notetype-ops.md T2): a non-empty name, field list and template list.
fn notetype_spec(notetype: &super::model::NotetypeSpec) -> Result<(), FormatError> {
    let empty = |what: &str| Err(FormatError::new(format!("{what} must not be empty")));
    if notetype.name.is_empty() {
        return empty("a note type name");
    }
    if notetype.fields.is_empty() {
        return empty(&format!("note type {:?}'s field list", notetype.name));
    }
    if notetype.templates.is_empty() {
        return empty(&format!("note type {:?}'s template list", notetype.name));
    }
    Ok(())
}

/// notetype-ops.md T2-T5: what the schema requires beyond the types (note
/// types named by non-empty names).
fn notetype_op(step: &Step) -> Result<(), FormatError> {
    let named = |names: &[Option<&String>]| {
        if names.iter().flatten().any(|name| name.is_empty()) {
            return Err(FormatError::new(format!(
                "{}: a note type name must not be empty",
                step.name()
            )));
        }
        Ok(())
    };
    match step {
        Step::AddNotetype(spec) => notetype_spec(spec),
        Step::AddStockNotetype(args) => named(&[args.name.as_ref()]),
        Step::CloneNotetype(args) => named(&[Some(&args.notetype), args.name.as_ref()]),
        Step::UpdateNotetype(args) => named(&[Some(&args.notetype)]),
        _ => Ok(()),
    }
}

/// model-ops.md L6: an export with `legacy` is exactly
/// `{format: apkg, name: <package file name>, legacy: true}`. Other forms stay
/// draft (D7.2).
fn legacy_export(args: &DraftArgs) -> Result<(), FormatError> {
    if !args.0.contains_key("legacy") {
        return Ok(());
    }
    let exact = args.0.len() == 3
        && args.0.get("format").and_then(Value::as_str) == Some("apkg")
        && args.0.get("name").is_some_and(Value::is_string)
        && args.0.get("legacy") == Some(&Value::Bool(true));
    if !exact {
        return Err(FormatError::new(
            "a legacy export is exactly {format: apkg, name, legacy: true}",
        ));
    }
    package_name(&args.0["name"])
}

/// model-ops.md L6: a file in the session directory, `^[A-Za-z0-9][A-Za-z0-9._-]*\.apkg$`.
fn package_name(value: &Value) -> Result<(), FormatError> {
    let stem = value.as_str().and_then(|name| name.strip_suffix(".apkg"));
    let ok = stem.is_some_and(|stem| {
        stem.starts_with(|c: char| c.is_ascii_alphanumeric())
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    });
    if ok {
        Ok(())
    } else {
        Err(FormatError::new(format!(
            "{value} is not a package file name (letters, digits, '.', '_' and '-', starting with a letter or digit, ending in .apkg)"
        )))
    }
}

/// The `col` columns `package_col` reads (model-ops.md L7).
const PACKAGE_COLUMNS: [&str; 6] = ["ver", "conf", "models", "decks", "dconf", "tags"];

/// The schema's rules for the list form of an observation request
/// (search-ops.md S1, S2).
pub(super) fn observation_list(kind: ObservationKind, items: &[Value]) -> Result<(), FormatError> {
    match kind {
        ObservationKind::ReviewHtml => super::ReviewHtmlCase::from_list(items).map(drop),
        ObservationKind::Search => SearchRequest::from_list(items).map(drop),
        ObservationKind::CanonicalSearch => canonical_search_texts(items).map(drop),
        other => Err(FormatError::new(format!(
            "observation {} does not take a list",
            other.name()
        ))),
    }
}

/// observations.md O2: a kind whose value is a list (`template`,
/// `fields_check`, `search`, `canonical_search`, `ignored_before_count`) is
/// requested at most once per step; two lists do not merge.
fn once_per_step(requests: &[super::model::ObservationRequest]) -> Result<(), FormatError> {
    for kind in [
        ObservationKind::ReviewHtml,
        ObservationKind::Template,
        ObservationKind::FieldsCheck,
        ObservationKind::Search,
        ObservationKind::CanonicalSearch,
        ObservationKind::CardExtractors,
        ObservationKind::CardExcerpts,
        ObservationKind::IgnoredBeforeCount,
        ObservationKind::PackageListing,
    ] {
        if requests.iter().filter(|r| r.kind == kind).count() > 1 {
            return Err(FormatError::new(format!(
                "observation {} is requested more than once in one step",
                kind.name()
            )));
        }
    }
    Ok(())
}

/// The schema's rules for the mapping form of an observation request.
pub(super) fn observation_mapping(kind: ObservationKind, map: &Map<String, Value>) -> Result<(), FormatError> {
    let name = kind.name();
    let bad = |why: &str| Err(FormatError::new(format!("observation {name}: {why}")));
    let uint = |v: &Value| v.is_u64();
    let only = |keys: &[&str]| map.keys().all(|k| keys.contains(&k.as_str()));
    match kind {
        ObservationKind::PackageListing => {
            super::PackageListingRequest::from_mapping(map)?;
        }
        ObservationKind::CardExtractors => {
            super::CardExtractorsRequest::from_mapping(map)?;
        }
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
        // model-ops.md L1 and L4.
        ObservationKind::LegacyJson | ObservationKind::BlobDecode => {
            let lists_ok = map.iter().all(|(key, names)| {
                matches!(key.as_str(), "notetypes" | "decks" | "presets")
                    && names.as_array().is_some_and(|names| {
                        !names.is_empty() && names.iter().all(|n| n.as_str().is_some_and(|n| !n.is_empty()))
                    })
            });
            if map.is_empty() || !lists_ok {
                return bad("takes {notetypes?, decks?, presets?}, each a non-empty list of names");
            }
        }
        // model-ops.md L7.
        ObservationKind::PackageCol => {
            if !only(&["name", "columns"]) || !map.contains_key("name") {
                return bad("takes {name, columns}");
            }
            package_name(&map["name"]).map_err(|e| FormatError::new(format!("observation {name}: {e}")))?;
            let Some(columns) = map.get("columns").and_then(Value::as_array).filter(|c| !c.is_empty()) else {
                return bad("columns is a non-empty list of col columns");
            };
            let mut seen = BTreeSet::new();
            for column in columns {
                let column = column.as_str().unwrap_or_default();
                if !PACKAGE_COLUMNS.contains(&column) {
                    return bad(&format!("columns are among {}", PACKAGE_COLUMNS.join(", ")));
                }
                if !seen.insert(column) {
                    return bad(&format!("column {column} is listed twice"));
                }
            }
        }
        ObservationKind::TypeAnswer => {
            super::TypeAnswerRequest::from_mapping(map)?;
        }
        ObservationKind::Text => {
            TextRequest::from_mapping(map)?;
        }
        // cloze-ops.md Z1.
        ObservationKind::ImageOcclusion => {
            super::ImageOcclusionRequest::from_mapping(map)?;
        }
        ObservationKind::Cloze => {
            ClozeRequest::from_mapping(map)?;
        }
        ObservationKind::FsrsNext => {
            FsrsNextRequest::from_mapping(map)?;
        }
        // deck-options-ops.md E2.
        ObservationKind::IgnoredBeforeCount => {
            IgnoredBeforeCountRequest::from_mapping(map)?;
        }
        // render-ops.md Q1.1-Q1.3, Q1.6.
        ObservationKind::Template => {
            TemplateRequest::from_mapping(map)?;
        }
        // note-ops.md N4.
        ObservationKind::FieldsCheck => {
            FieldsCheckRequest::from_mapping(map)?;
        }
        ObservationKind::DbDump => {
            if let Some(key) = map.keys().find(|k| !matches!(k.as_str(), "tables" | "raw")) {
                return bad(&format!("takes {{tables, raw}}, not {key:?}"));
            }
            if map.get("raw").is_some_and(|raw| !raw.is_boolean()) {
                return bad("raw is a boolean");
            }
            if let Some(tables) = map.get("tables") {
                let Some(tables) = tables.as_array().filter(|t| !t.is_empty()) else {
                    return bad("tables is a non-empty list of table names");
                };
                let mut seen = BTreeSet::new();
                for table in tables {
                    let name = table.as_str().unwrap_or_default();
                    if !DUMP_TABLES.contains(&name) {
                        return bad(&format!("{table} is not one of {}", DUMP_TABLES.join(", ")));
                    }
                    if !seen.insert(name) {
                        return bad(&format!("table {name} is listed twice"));
                    }
                }
            }
        }
        // notetype-ops.md T8.
        ObservationKind::NotetypeStructure => {
            let names_ok = map.get("notetypes").and_then(Value::as_array).is_some_and(|names| {
                !names.is_empty() && names.iter().all(|n| n.as_str().is_some_and(|n| !n.is_empty()))
            });
            if map.len() != 1 || !names_ok {
                return bad("takes {notetypes}, a non-empty list of note type names");
            }
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
