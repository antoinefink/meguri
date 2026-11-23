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

pub(crate) const PAGE: usize = 1000;
/// Notes per page: with `LIMITS.note_bytes` a page holds 128 MiB at most.
pub(crate) const NOTE_PAGE: usize = 64;

/// What the plan keeps of one package note: never its text.
#[derive(Clone, Debug)]
pub(crate) struct NoteHead {
    pub id: i64,
    pub guid: Vec<u8>,
    pub notetype: i64,
    pub mtime: i64,
}

/// What an apply keeps in memory of the package collection, as I3 left it:
/// each note's identity and what the cards refer to. Field text, cards and
/// review-log rows are read again in pages when they are needed (R1852), so
/// memory grows by a few dozen bytes per note, never by the package's text.
pub(crate) struct RawPackage {
    /// In ascending id.
    pub notes: Vec<NoteHead>,
    /// Cards whose note the package holds, and the review-log rows read
    /// with scheduling (I6): the write phase's row estimate.
    pub cards: u64,
    pub revlog: u64,
    /// Every `did` a taken card names; those named with `odid` 0; every
    /// non-zero `odid`.
    pub card_decks: BTreeSet<i64>,
    pub home_decks: BTreeSet<i64>,
    pub original_decks: BTreeSet<i64>,
    pub notetypes: BTreeMap<i64, Notetype>,
    pub decks: Vec<Deck>,
    pub presets: Vec<DeckConfig>,
    pub next_position: u32,
    pub created: i64,
    pub creation_offset: Option<i32>,
    pub rollover_hour: u8,
}

/// The staged package collection, opened for one pass over its rows.
pub(crate) struct PackageRows {
    opened: Option<meguri_storage::OpenedCollection>,
}

impl PackageRows {
    pub fn open(path: &Path, context: &ApkgContext) -> ImportOutcome<PackageRows> {
        let opened =
            meguri_storage::deck_package::open_deck_candidate(path, context.clock.as_ref(), context.random.as_ref())
                .map_err(|gate| corrupt(format!("candidate gate: {gate}")))?;
        Ok(PackageRows { opened: Some(opened) })
    }

    pub fn rows(&self) -> &meguri_storage::Writer {
        self.opened.as_ref().expect("open until dropped").writer()
    }

    /// Up to a page of notes above `after`, by id.
    pub fn notes_after(&self, after: Option<i64>) -> ImportOutcome<Vec<Note>> {
        self.rows().notes_after(after.map(meguri_model::ids::NoteId), NOTE_PAGE).map_err(unreadable)
    }

    pub fn cards_after(&self, after: Option<i64>) -> ImportOutcome<Vec<Card>> {
        self.rows().cards_after(after.map(meguri_model::ids::CardId), PAGE).map_err(unreadable)
    }

    pub fn revlog_after(&self, after: Option<i64>) -> ImportOutcome<Vec<RevlogEntry>> {
        self.rows()
            .revlog_after(after.map(meguri_model::ids::RevlogId), PAGE)
            .map_err(unreadable)
    }
}

impl Drop for PackageRows {
    fn drop(&mut self) {
        if let Some(opened) = self.opened.take() {
            let _ = opened.close();
        }
    }
}

pub(crate) fn read(
    path: &Path,
    context: &ApkgContext,
    scheduling: bool,
    check: &dyn Fn() -> ImportOutcome<()>,
) -> ImportOutcome<RawPackage> {
    let package = PackageRows::open(path, context)?;
    let rows = package.rows();
    let mut notes: Vec<NoteHead> = Vec::new();
    loop {
        check()?;
        let after = notes.last().map(|note| meguri_model::ids::NoteId(note.id));
        let page = rows.note_identities_after(after, 5000).map_err(unreadable)?;
        if page.is_empty() {
            break;
        }
        notes.extend(page.into_iter().map(|identity| NoteHead {
            id: identity.id.0,
            guid: identity.guid,
            notetype: identity.notetype.0,
            mtime: identity.mtime,
        }));
    }
    // I6 takes the cards of the package's notes and the review log of
    // those cards: rows that name no parent play no part.
    let note_ids: HashSet<i64> = notes.iter().map(|note| note.id).collect();
    let (mut card_decks, mut home_decks, mut original_decks) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut cards = 0_u64;
    let mut after = None;
    loop {
        check()?;
        let page = package.cards_after(after)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id.0);
        for card in &page {
            if !note_ids.contains(&card.note_id.0) {
                continue;
            }
            cards += 1;
            card_decks.insert(card.deck_id.0);
            if card.original_deck_id.0 == 0 {
                home_decks.insert(card.deck_id.0);
            } else {
                original_decks.insert(card.original_deck_id.0);
            }
        }
    }
    drop(note_ids);
    let mut notetypes = BTreeMap::new();
    for id in notes.iter().map(|note| note.notetype).collect::<BTreeSet<_>>() {
        if let Some(notetype) = rows.notetype(NotetypeId(id)).map_err(unreadable)? {
            notetypes.insert(id, notetype);
        }
    }
    let config = SchedConfig::new(rows);
    Ok(RawPackage {
        notes,
        cards,
        // Without scheduling no review-log row is taken (I6).
        revlog: if scheduling { rows.row_count(meguri_storage::Table::Revlog).map_err(unreadable)? } else { 0 },
        card_decks,
        home_decks,
        original_decks,
        notetypes,
        decks: rows.deck_rows().map_err(unreadable)?,
        presets: rows.deck_config_rows().map_err(unreadable)?,
        next_position: meguri_engine::cardgen::position::read_counter(rows)?,
        created: rows.col().map_err(unreadable)?.created.0,
        creation_offset: config.creation_offset().map_err(unreadable)?,
        rollover_hour: config.rollover_hour().map_err(unreadable)?,
    })
}

/// What an import with given options takes besides notes and cards: I6's
/// note types, decks and presets, and how each card is changed when its
/// page is read.
pub(crate) struct Taken {
    /// By package id, ascending; `original_id` set to the package id (I7).
    pub notetypes: Vec<Notetype>,
    /// In I10.2's order: by name depth, then package id.
    pub decks: Vec<Deck>,
    pub presets: Vec<DeckConfig>,
    scheduling: bool,
    /// A5: a card could not keep its place, so held cards go home.
    return_home: bool,
    first_position: u32,
}

/// A6's running position counter over the taken cards, in ascending id.
pub(crate) struct CardShaper<'t> {
    taken: &'t Taken,
    next: u32,
}

impl Taken {
    pub fn shaper(&self) -> CardShaper<'_> {
        CardShaper {
            taken: self,
            next: self.first_position,
        }
    }

    /// A note's tags as I6 leaves them: without scheduling, `marked` and
    /// `leech` go.
    pub fn tags(&self, stored: &str) -> String {
        let kept: Vec<&str> = meguri_engine::tags::canonical::split_text(stored)
            .filter(|tag| self.scheduling || !(tag.eq_ignore_ascii_case("marked") || tag.eq_ignore_ascii_case("leech")))
            .collect();
        kept.join(" ")
    }
}

impl CardShaper<'_> {
    /// A5 or A6 on one taken card. Every taken card passes through here in
    /// ascending id, whether or not it is added afterwards.
    pub fn shape(&mut self, card: &mut Card) {
        if self.taken.scheduling {
            if self.taken.return_home && card.original_deck_id.0 > 0 {
                card.original_deck_id = DeckId(0);
                if card.original_due != 0 {
                    card.due = card.original_due;
                }
                if card.queue.stored() >= 0 {
                    card.queue = meguri_sched::model::card_sched::queue_for_type(card.card_type, card.due);
                }
                card.original_due = 0;
            }
            return;
        }
        let last = if card.card_type == CardType::New {
            Some(if card.original_deck_id.0 > 0 { card.original_due } else { card.due } as u32)
        } else {
            card.data.new_position
        };
        let position = last.unwrap_or_else(|| {
            let taken = self.next;
            self.next = self.next.wrapping_add(1);
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
    let mut set: BTreeSet<i64> = raw.card_decks.iter().copied().filter(|id| by_id.contains_key(id)).collect();
    if scheduling {
        set.extend(raw.original_decks.iter().copied().filter(|id| by_id.contains_key(id)));
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

    let mut return_home = false;
    if scheduling {
        // A5: filtered decks stay unless a card cannot keep its place.
        let normal_in_set = |id: &i64| set.contains(id) && !is_filtered(by_id[id]);
        let keep = raw.home_decks.iter().all(normal_in_set)
            && raw.original_decks.iter().all(|id| *id == 1 || set.contains(id));
        if !keep {
            return_home = true;
            for deck in &mut decks {
                if is_filtered(deck) {
                    deck.kind = normal_default();
                }
            }
        }
    } else {
        // A6, the decks' part; the cards' is `CardShaper`.
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
        scheduling,
        return_home,
        first_position: raw.next_position,
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
