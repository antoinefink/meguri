//! Test ops that record each kind of undo step in a session (answer, bury,
//! suspend, edit), standing in for U44's answer and U48's card ops, and a
//! whole-collection dump to compare states (`specs/engine/undo.md`).

use std::collections::BTreeMap;

use meguri_engine::{Collection, EngineError, OpResult, OpSpec, SessionId, StepKind};
use meguri_model::card::{CardQueue, CardType};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, NoteId, RevlogId};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole};

pub use meguri_engine::undo::names::{ANSWER_CARD, BURY, SUSPEND, UPDATE_NOTE};

fn spec(op: &'static str, step: &'static str, kind: StepKind, session: Option<SessionId>) -> OpSpec {
    let spec = OpSpec::named(op, OpSource::UserUndoable).undo_step(step, kind);
    match session {
        Some(session) => spec.session(session),
        None => spec,
    }
}

/// A note with one card, added by an op that leaves the history alone
/// (skip-undo), so tests start from an untouched history.
pub fn setup_note(c: &Collection, front: &str) -> (NoteId, CardId) {
    let note = super::note(&[front, "back"]);
    c.run(
        OpSpec::named("setup_note", OpSource::UserNonUndoable).skip_undo(),
        move |ctx| {
            let nid = ctx.scope().add_note(&note)?;
            let cid = ctx.scope().add_card(&super::card(nid, 0))?;
            Ok((nid, cid))
        },
    )
    .unwrap()
    .value
}

/// An answer: the card's next state and its review-log row.
pub fn answer(c: &Collection, session: Option<SessionId>, card: CardId) -> Result<OpResult<RevlogId>, EngineError> {
    c.run(
        spec("answer_card", ANSWER_CARD, StepKind::Answer, session),
        move |ctx| {
            let mut stored = ctx.scope().card(card)?.expect("the card exists");
            stored.card_type = CardType::Review;
            stored.queue = CardQueue::Review;
            stored.reps += 1;
            stored.due += 1;
            stored.interval += 1;
            ctx.scope().put(&stored)?;
            Ok(ctx.scope().add_revlog(&super::revlog(card))?)
        },
    )
}

/// Buries a card by hand.
pub fn bury(c: &Collection, session: Option<SessionId>, card: CardId) -> Result<OpResult<()>, EngineError> {
    set_queue(
        c,
        spec("bury_cards", BURY, StepKind::Other, session),
        card,
        CardQueue::BuriedByUser,
    )
}

pub fn suspend(c: &Collection, session: Option<SessionId>, card: CardId) -> Result<OpResult<()>, EngineError> {
    set_queue(
        c,
        spec("suspend_cards", SUSPEND, StepKind::Other, session),
        card,
        CardQueue::Suspended,
    )
}

fn set_queue(c: &Collection, spec: OpSpec, card: CardId, queue: CardQueue) -> Result<OpResult<()>, EngineError> {
    c.run(spec, move |ctx| {
        let mut stored = ctx.scope().card(card)?.expect("the card exists");
        stored.queue = queue;
        ctx.scope().put(&stored)?;
        Ok(())
    })
}

/// Sets a note's first field.
pub fn edit(
    c: &Collection,
    session: Option<SessionId>,
    note: NoteId,
    value: &str,
) -> Result<OpResult<()>, EngineError> {
    let value = value.to_owned();
    c.run(spec("update_note", UPDATE_NOTE, StepKind::Other, session), move |ctx| {
        set_first_field(ctx, note, value)
    })
}

pub fn set_first_field(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    note: NoteId,
    value: String,
) -> Result<(), EngineError> {
    let mut stored = ctx.scope().note(note)?.expect("the note exists");
    let mut fields: Vec<String> = stored.field_values().map(str::to_owned).collect();
    fields[0] = value.clone();
    stored.fields = fields.join("\u{1f}");
    stored.sort_field = value;
    ctx.scope().put(&stored)?;
    Ok(())
}

/// Every row of every table, by table name, in key order.
pub type Dump = BTreeMap<&'static str, Vec<Vec<SqlValue>>>;

const TABLES: [(&str, &str); 12] = [
    ("col", "id"),
    ("notes", "id"),
    ("cards", "id"),
    ("revlog", "id"),
    ("graves", "oid, type"),
    ("deck_config", "id"),
    ("config", "KEY"),
    ("tags", "tag"),
    ("fields", "ntid, ord"),
    ("templates", "ntid, ord"),
    ("notetypes", "id"),
    ("decks", "id"),
];

pub fn dump(c: &Collection) -> Dump {
    let reader = c.reader(ReaderRole::Query).unwrap();
    TABLES
        .iter()
        .map(|(table, key)| {
            let rows = reader
                .execute_raw(&format!("SELECT * FROM {table} ORDER BY {key}"), &[])
                .unwrap()
                .rows;
            (*table, rows)
        })
        .collect()
}

/// The rows whose content differs between two dumps, as (table, first
/// column or first two for two-column keys).
pub fn changed_rows(a: &Dump, b: &Dump) -> Vec<(&'static str, Vec<SqlValue>)> {
    let mut changed = Vec::new();
    for (table, key_len) in TABLES.iter().map(|(t, k)| (*t, k.split(',').count())) {
        let keyed = |rows: &Vec<Vec<SqlValue>>| -> BTreeMap<String, Vec<SqlValue>> {
            rows.iter()
                .map(|row| (format!("{:?}", &row[..key_len]), row.clone()))
                .collect()
        };
        let (before, after) = (keyed(&a[table]), keyed(&b[table]));
        for (key, row) in &before {
            if after.get(key) != Some(row) {
                changed.push((table, row[..key_len].to_vec()));
            }
        }
        for (key, row) in &after {
            if !before.contains_key(key) {
                changed.push((table, row[..key_len].to_vec()));
            }
        }
    }
    changed
}
