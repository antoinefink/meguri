//! What an import takes from the package collection (`package/apkg-import`
//! I6, with `package/apkg-export` A3, A5, A6 and A7 standing for an export
//! of every note): rows read once, then changed in memory only.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use meguri_engine::decks::names;
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck};
use meguri_model::revlog::RevlogEntry;
use meguri_sched::model::sched_config::SchedConfig;
use meguri_storage::ReadRows;

use super::session::unreadable;
use super::{ApkgContext, ImportOptions, ImportOutcome, corrupt};

const PAGE: usize = 2000;

/// The package collection's rows, as I3 left them.
pub(crate) struct RawPackage {
    pub notes: Vec<Note>,
    pub cards: Vec<Card>,
    pub revlog: Vec<RevlogEntry>,
    pub notetypes: BTreeMap<i64, Notetype>,
    pub decks: Vec<Deck>,
    pub presets: Vec<DeckConfig>,
    pub next_position: u32,
    pub created: i64,
    pub creation_offset: Option<i32>,
    pub rollover_hour: u8,
}

pub(crate) fn read(
    path: &Path,
    context: &ApkgContext,
    scheduling: bool,
    check: &dyn Fn() -> ImportOutcome<()>,
) -> ImportOutcome<RawPackage> {
    let opened =
        meguri_storage::deck_package::open_deck_candidate(path, context.clock.as_ref(), context.random.as_ref())
            .map_err(|gate| corrupt(format!("candidate gate: {gate}")))?;
    let outcome = read_rows(opened.writer(), scheduling, check);
    let closed = opened.close();
    let raw = outcome?;
    closed?;
    Ok(raw)
}

fn read_rows(
    rows: &impl ReadRows,
    scheduling: bool,
    check: &dyn Fn() -> ImportOutcome<()>,
) -> ImportOutcome<RawPackage> {
    let mut notes = Vec::new();
    loop {
        check()?;
        let page = rows.notes_after(notes.last().map(|n: &Note| n.id), PAGE).map_err(unreadable)?;
        if page.is_empty() {
            break;
        }
        notes.extend(page);
    }
    let mut cards = Vec::new();
    loop {
        check()?;
        let page = rows.cards_after(cards.last().map(|c: &Card| c.id), PAGE).map_err(unreadable)?;
        if page.is_empty() {
            break;
        }
        cards.extend(page);
    }
    // I6 takes the cards of the package's notes and the review log of
    // those cards: rows that name no parent play no part.
    let note_ids: HashSet<i64> = notes.iter().map(|note| note.id.0).collect();
    cards.retain(|card| note_ids.contains(&card.note_id.0));
    let mut revlog = Vec::new();
    // Without scheduling no review-log row is taken (I6): none is read.
    if scheduling {
        loop {
            check()?;
            let page = rows
                .revlog_after(revlog.last().map(|r: &RevlogEntry| r.id), PAGE)
                .map_err(unreadable)?;
            if page.is_empty() {
                break;
            }
            revlog.extend(page);
        }
    }
    let card_ids: HashSet<i64> = cards.iter().map(|card| card.id.0).collect();
    revlog.retain(|row| card_ids.contains(&row.card_id.0));
    let mut notetypes = BTreeMap::new();
    for id in notes.iter().map(|note| note.notetype_id.0).collect::<BTreeSet<_>>() {
        if let Some(notetype) = rows.notetype(NotetypeId(id)).map_err(unreadable)? {
            notetypes.insert(id, notetype);
        }
    }
    let config = SchedConfig::new(rows);
    Ok(RawPackage {
        notes,
        cards,
        revlog,
        notetypes,
        decks: rows.deck_rows().map_err(unreadable)?,
        presets: rows.deck_config_rows().map_err(unreadable)?,
        next_position: meguri_engine::cardgen::position::read_counter(rows)?,
        created: rows.col().map_err(unreadable)?.created.0,
        creation_offset: config.creation_offset().map_err(unreadable)?,
        rollover_hour: config.rollover_hour().map_err(unreadable)?,
    })
}

/// The rows an import with given options writes from: I6's result.
pub(crate) struct Taken {
    /// By package id, ascending; `original_id` set to the package id (I7).
    pub notetypes: Vec<Notetype>,
    /// In I10.2's order: by name depth, then package id.
    pub decks: Vec<Deck>,
    pub presets: Vec<DeckConfig>,
    /// Every card, ascending id, as scheduling on or off leaves it.
    pub cards: Vec<Card>,
    /// With scheduling only.
    pub revlog: Vec<RevlogEntry>,
    /// Per note index: its tags as I6 leaves them.
    pub tags: Vec<String>,
}

fn normal_default() -> ProtoBlob<DeckKindMessage> {
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal)))
}

fn is_filtered(deck: &Deck) -> bool {
    matches!(deck.kind.kind, DeckKind::Filtered(_))
}

pub(crate) fn take(raw: &RawPackage, options: &ImportOptions) -> Taken {
    let scheduling = options.with_scheduling;
    // A3: the deck set.
    let by_id: HashMap<i64, &Deck> = raw.decks.iter().map(|deck| (deck.id.0, deck)).collect();
    let mut set: BTreeSet<i64> = BTreeSet::new();
    for card in &raw.cards {
        if by_id.contains_key(&card.deck_id.0) {
            set.insert(card.deck_id.0);
        }
        if scheduling && card.original_deck_id.0 != 0 && by_id.contains_key(&card.original_deck_id.0) {
            set.insert(card.original_deck_id.0);
        }
    }
    for id in set.clone() {
        let mut name = by_id[&id].name.as_str();
        while let Some(parent) = names::parent(name) {
            let Some(found) = names::find(&raw.decks, parent) else { break };
            if !set.insert(found.id.0) {
                break;
            }
            name = found.name.as_str();
        }
    }
    if !scheduling {
        set.remove(&1);
    }

    let mut cards = raw.cards.clone();
    let mut decks: Vec<Deck> = set.iter().map(|id| by_id[id].clone()).collect();
    // A7: presets named by the normal decks, as read.
    let preset_ids: BTreeSet<i64> = raw.presets.iter().map(|preset| preset.id.0).collect();
    let mut presets = Vec::new();
    if options.with_deck_configs {
        let wanted: BTreeSet<i64> = decks
            .iter()
            .filter_map(|deck| match &deck.kind.kind {
                DeckKind::Normal(normal) => Some(normal.config_id),
                DeckKind::Filtered(_) => None,
            })
            .collect();
        for preset in &raw.presets {
            if wanted.contains(&preset.id.0) {
                let mut preset = preset.clone();
                if !scheduling {
                    preset.config.clear_fsrs_params();
                }
                presets.push(preset);
            }
        }
        presets.sort_by_key(|preset| preset.id.0);
    }

    if scheduling {
        // A5: filtered decks stay unless a card cannot keep its place.
        let normal_in_set = |id: i64| set.contains(&id) && !is_filtered(by_id[&id]);
        let keep = cards.iter().all(|card| {
            if card.original_deck_id.0 == 0 {
                normal_in_set(card.deck_id.0)
            } else {
                card.original_deck_id.0 == 1 || set.contains(&card.original_deck_id.0)
            }
        });
        if !keep {
            for card in &mut cards {
                if card.original_deck_id.0 > 0 {
                    card.original_deck_id = DeckId(0);
                    if card.original_due != 0 {
                        card.due = card.original_due;
                    }
                    if card.queue.stored() >= 0 {
                        card.queue = meguri_sched::model::card_sched::queue_for_type(card.card_type, card.due);
                    }
                    card.original_due = 0;
                }
            }
            for deck in &mut decks {
                if is_filtered(deck) {
                    deck.kind = normal_default();
                }
            }
        }
    } else {
        // A6.
        let mut next = raw.next_position;
        for card in &mut cards {
            let last = if card.card_type == CardType::New {
                Some(if card.original_deck_id.0 > 0 { card.original_due } else { card.due } as u32)
            } else {
                card.data.new_position
            };
            let position = last.unwrap_or_else(|| {
                let taken = next;
                next = next.wrapping_add(1);
                taken
            });
            card.due = position as i32;
            card.card_type = CardType::New;
            card.queue = CardQueue::New;
            card.interval = 0;
            card.ease_factor = 0;
            card.reps = 0;
            card.lapses = 0;
            if card.original_deck_id.0 != 0 {
                card.original_due = 0;
            }
            card.original_deck_id = DeckId(0);
            card.flags = 0;
            card.data.new_position = None;
            card.data.memory_state = None;
        }
        for deck in &mut decks {
            deck.common = ProtoBlob::new(DeckCommon::default());
            match &mut deck.kind.kind {
                DeckKind::Normal(normal) => {
                    normal.extend_new = 0;
                    normal.extend_review = 0;
                    normal.review_limit = None;
                    normal.new_limit = None;
                    normal.review_limit_today = None;
                    normal.new_limit_today = None;
                }
                DeckKind::Filtered(_) => deck.kind = normal_default(),
            }
        }
    }
    // A7 with I6's difference: an unknown or untaken preset reads as 1.
    let taken_presets: HashSet<i64> = presets.iter().map(|preset| preset.id.0).collect();
    for deck in &mut decks {
        if let DeckKind::Normal(normal) = &mut deck.kind.kind
            && normal.config_id != 1
            && !(options.with_deck_configs && preset_ids.contains(&normal.config_id) && taken_presets.contains(&normal.config_id))
        {
            normal.config_id = 1;
        }
    }
    decks.sort_by_key(|deck| (deck.name.split('\u{1f}').count(), deck.id.0));

    let tags = raw
        .notes
        .iter()
        .map(|note| {
            let kept: Vec<&str> = meguri_engine::tags::canonical::split_text(&note.tags)
                .filter(|tag| scheduling || !(tag.eq_ignore_ascii_case("marked") || tag.eq_ignore_ascii_case("leech")))
                .collect();
            kept.join(" ")
        })
        .collect();

    let notetypes = raw
        .notetypes
        .values()
        .map(|notetype| {
            let mut notetype = notetype.clone();
            notetype.config.original_id = Some(notetype.id.0);
            notetype
        })
        .collect();
    Taken {
        notetypes,
        decks,
        presets,
        cards,
        revlog: if scheduling { raw.revlog.clone() } else { Vec::new() },
        tags,
    }
}

/// The package collection's today (I11.2's P), writing nothing.
pub(crate) fn package_today(raw: &RawPackage, context: &ApkgContext, now: i64) -> u32 {
    meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
        crt: raw.created,
        creation_offset: raw.creation_offset,
        offset: context.clock.utc_offset_at(now),
        rollover_hour: raw.rollover_hour,
        now: now.div_euclid(1000),
    })
    .today
}
