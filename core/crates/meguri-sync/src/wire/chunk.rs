//! `chunk` and `applyChunk` (specs/sync/wire-format.md W7 to W11; R1529):
//! the chunk object, the card, note and review tuples, and the split of a
//! sync's pending records into chunks of at most 250 objects.

use std::collections::VecDeque;

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::note::Note;
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use serde_json::value::RawValue;

use super::read::{self, At};
use super::write;
use crate::error::JobError;
use crate::transport::Method;

/// The most objects one chunk holds, its three lists together (W7.2).
pub const OBJECTS_PER_CHUNK: usize = 250;

/// What the client sends in one `applyChunk` (W7.1). Every tuple goes out
/// with the server's `usn` from this sync's `meta` (W6.2, W8).
#[derive(Debug, Clone, Copy)]
pub struct OutgoingChunk<'a> {
    pub done: bool,
    pub revlog: &'a [RevlogEntry],
    pub cards: &'a [Card],
    pub notes: &'a [Note],
}

/// The `applyChunk` request: `{"chunk":<chunk object>}`, empty lists left
/// out.
pub fn encode_apply_chunk(chunk: &OutgoingChunk<'_>, server_usn: Usn) -> Vec<u8> {
    let mut out = String::from("{\"chunk\":{\"done\":");
    write::boolean(&mut out, chunk.done);
    if !chunk.revlog.is_empty() {
        out.push_str(",\"revlog\":[");
        for (index, entry) in chunk.revlog.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_review(&mut out, entry, server_usn);
        }
        out.push(']');
    }
    if !chunk.cards.is_empty() {
        out.push_str(",\"cards\":[");
        for (index, card) in chunk.cards.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_card(&mut out, card, server_usn);
        }
        out.push(']');
    }
    if !chunk.notes.is_empty() {
        out.push_str(",\"notes\":[");
        for (index, note) in chunk.notes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_note(&mut out, note, server_usn);
        }
        out.push(']');
    }
    out.push_str("}}");
    out.into_bytes()
}

/// W8: the `cards` row in column order, `data` rebuilt from the card's
/// values (W8.1, `model/card-data-json` C4).
fn write_card(out: &mut String, card: &Card, server_usn: Usn) {
    out.push('[');
    for value in [
        card.id.0,
        card.note_id.0,
        card.deck_id.0,
        i64::from(card.ord),
        card.mtime.0,
        i64::from(server_usn.0),
        card.card_type.stored(),
        card.queue.stored(),
        i64::from(card.due),
        i64::from(card.interval),
        i64::from(card.ease_factor),
        i64::from(card.reps),
        i64::from(card.lapses),
        i64::from(card.remaining_steps),
        i64::from(card.original_due),
        card.original_deck_id.0,
        i64::from(card.flags),
    ] {
        write::int(out, value);
        out.push(',');
    }
    write::string(out, &card.data.to_text());
    out.push(']');
}

/// W9: elements 8 to 11 are always `""`, `""`, 0 and `""`.
fn write_note(out: &mut String, note: &Note, server_usn: Usn) {
    out.push('[');
    write::int(out, note.id.0);
    out.push(',');
    write::string(out, &note.guid);
    out.push(',');
    write::int(out, note.notetype_id.0);
    out.push(',');
    write::int(out, note.mtime.0);
    out.push(',');
    write::int(out, server_usn.0);
    out.push(',');
    write::string(out, &wire_tags(&note.tags));
    out.push(',');
    write::string(out, &note.fields);
    out.push_str(",\"\",\"\",0,\"\"]");
}

/// W10: the `revlog` row, `type` included.
fn write_review(out: &mut String, entry: &RevlogEntry, server_usn: Usn) {
    write::ints(
        out,
        &[
            entry.id.0,
            entry.card_id.0,
            i64::from(server_usn.0),
            i64::from(entry.button),
            i64::from(entry.interval),
            i64::from(entry.last_interval),
            i64::from(entry.ease_factor),
            i64::from(entry.answer_millis),
            entry.kind.stored(),
        ],
    );
}

/// W9.1: the tags of a stored `tags` column, split at every space (U+0020)
/// and ideographic space (U+3000), empty pieces dropped.
pub fn note_tags(stored: &str) -> Vec<&str> {
    stored.split([' ', '\u{3000}']).filter(|tag| !tag.is_empty()).collect()
}

/// W9.1 written: the tags joined with one space and one space added at each
/// end; `""` without tags.
fn wire_tags(stored: &str) -> String {
    let tags = note_tags(stored);
    if tags.is_empty() {
        String::new()
    } else {
        format!(" {} ", tags.join(" "))
    }
}

/// A note as a chunk carries it (W9). The sort field, checksum, `flags` and
/// `data` travel as constants; a receiver recomputes the first two and
/// stores `flags` 0 and `data` empty (U107).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingNote {
    pub id: NoteId,
    pub guid: String,
    pub notetype_id: NotetypeId,
    pub mtime: EpochSecs,
    pub usn: Usn,
    /// Split per W9.1.
    pub tags: Vec<String>,
    /// The field values joined by U+001F.
    pub fields: String,
}

/// A chunk object, decoded (W7.1).
#[derive(Debug, Clone)]
pub struct IncomingChunk {
    pub done: bool,
    pub revlog: Vec<RevlogEntry>,
    /// The `data` text is read per `model/card-data-json` C3.
    pub cards: Vec<Card>,
    pub notes: Vec<IncomingNote>,
}

/// The `chunk` answer (W7.1).
pub fn decode_chunk(body: &[u8]) -> Result<IncomingChunk, JobError> {
    decode(body).map_err(|err| JobError::bad_response(Method::Chunk, err.to_string()))
}

fn decode(body: &[u8]) -> read::Result<IncomingChunk> {
    let at = At::Root;
    let root = read::object(read::body(body)?, at)?;
    let done = match root.get("done", at)? {
        Some(raw) => read::boolean(raw, at.member("done"))?,
        None => false,
    };
    let list = |key: &'static str| -> read::Result<Vec<&RawValue>> {
        match root.get(key, at)? {
            Some(raw) => read::array(raw, at.member(key)),
            None => Ok(Vec::new()),
        }
    };
    let revlog_at = at.member("revlog");
    let revlog = list("revlog")?
        .into_iter()
        .enumerate()
        .map(|(index, raw)| review(raw, revlog_at.index(index)))
        .collect::<read::Result<Vec<_>>>()?;
    let cards_at = at.member("cards");
    let cards = list("cards")?
        .into_iter()
        .enumerate()
        .map(|(index, raw)| card(raw, cards_at.index(index)))
        .collect::<read::Result<Vec<_>>>()?;
    let notes_at = at.member("notes");
    let notes = list("notes")?
        .into_iter()
        .enumerate()
        .map(|(index, raw)| note(raw, notes_at.index(index)))
        .collect::<read::Result<Vec<_>>>()?;
    Ok(IncomingChunk {
        done,
        revlog,
        cards,
        notes,
    })
}

/// W8: eighteen elements.
fn card(raw: &RawValue, at: At<'_>) -> read::Result<Card> {
    let items = read::tuple(raw, 18, 18, at)?;
    let item = |index: usize| (items[index], at.index(index));
    let strict_i64 = |index: usize| {
        let (raw, at) = item(index);
        read::strict::<i64>(raw, at)
    };
    let loose = |index: usize| {
        let (raw, at) = item(index);
        read::loose(raw, at)
    };
    let (raw, type_at) = item(6);
    let card_type = match read::strict::<i64>(raw, type_at)? {
        0 => CardType::New,
        1 => CardType::Learning,
        2 => CardType::Review,
        3 => CardType::Relearning,
        _ => return Err(type_at.fail("is not a card type (0 to 3)")),
    };
    let (raw, queue_at) = item(7);
    let queue = match read::strict::<i64>(raw, queue_at)? {
        -3 => CardQueue::BuriedByUser,
        -2 => CardQueue::BuriedByScheduler,
        -1 => CardQueue::Suspended,
        0 => CardQueue::New,
        1 => CardQueue::Learning,
        2 => CardQueue::Review,
        3 => CardQueue::InterdayLearning,
        4 => CardQueue::Preview,
        _ => return Err(queue_at.fail("is not a card queue (-3 to 4)")),
    };
    let (raw, data_at) = item(17);
    let data = read::string(raw, data_at)?;
    Ok(Card {
        id: CardId(strict_i64(0)?),
        note_id: NoteId(strict_i64(1)?),
        deck_id: DeckId(strict_i64(2)?),
        ord: read::strict::<u16>(items[3], at.index(3))?,
        mtime: EpochSecs(loose(4)?),
        usn: Usn(read::strict::<i32>(items[5], at.index(5))?),
        card_type,
        queue,
        due: read::loose_i32(loose(8)?),
        interval: read::loose_u32(loose(9)?),
        ease_factor: read::strict::<u16>(items[10], at.index(10))?,
        reps: read::strict::<u32>(items[11], at.index(11))?,
        lapses: read::strict::<u32>(items[12], at.index(12))?,
        remaining_steps: read::strict::<u32>(items[13], at.index(13))?,
        original_due: read::loose_i32(loose(14)?),
        original_deck_id: DeckId(strict_i64(15)?),
        flags: read::strict::<u8>(items[16], at.index(16))?,
        data: CardData::parse(&data),
    })
}

/// W9: eleven elements; 8 to 11 are type-checked and ignored.
fn note(raw: &RawValue, at: At<'_>) -> read::Result<IncomingNote> {
    let items = read::tuple(raw, 11, 11, at)?;
    let tags = read::string(items[5], at.index(5))?;
    for index in [7, 8, 10] {
        read::string(items[index], at.index(index))?;
    }
    read::strict::<u32>(items[9], at.index(9))?;
    Ok(IncomingNote {
        id: NoteId(read::strict::<i64>(items[0], at.index(0))?),
        guid: read::string(items[1], at.index(1))?,
        notetype_id: NotetypeId(read::strict::<i64>(items[2], at.index(2))?),
        mtime: EpochSecs(read::strict::<i64>(items[3], at.index(3))?),
        usn: Usn(read::strict::<i32>(items[4], at.index(4))?),
        tags: note_tags(&tags).into_iter().map(str::to_owned).collect(),
        fields: read::string(items[6], at.index(6))?,
    })
}

/// W10: nine elements, or eight with `type` missing.
fn review(raw: &RawValue, at: At<'_>) -> read::Result<RevlogEntry> {
    let items = read::tuple(raw, 8, 9, at)?;
    let loose = |index: usize| read::loose(items[index], at.index(index));
    // W10.1: anything but an integer from 0 to 5 reads as 0.
    let kind = items
        .get(8)
        .and_then(|raw| read::strict::<u8>(raw, at.index(8)).ok())
        .filter(|value| *value <= 5)
        .map_or(RevlogKind::Learning, |value| RevlogKind::from_stored(i64::from(value)));
    Ok(RevlogEntry {
        id: RevlogId(read::strict::<i64>(items[0], at.index(0))?),
        card_id: CardId(read::strict::<i64>(items[1], at.index(1))?),
        usn: Usn(read::strict::<i32>(items[2], at.index(2))?),
        button: read::strict::<u8>(items[3], at.index(3))?,
        interval: read::loose_i32(loose(4)?),
        last_interval: read::loose_i32(loose(5)?),
        ease_factor: read::loose_u32(loose(6)?),
        answer_millis: read::loose_u32(loose(7)?),
        kind,
    })
}

/// The ids of one chunk to send (W7.2), and whether it is the last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkIds {
    pub revlog: Vec<RevlogId>,
    pub notes: Vec<NoteId>,
    pub cards: Vec<CardId>,
    /// True exactly when the chunk holds fewer than 250 objects.
    pub done: bool,
}

impl ChunkIds {
    pub fn len(&self) -> usize {
        self.revlog.len() + self.notes.len() + self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A sync's pending ids, handed out chunk by chunk (W7.2).
#[derive(Debug, Clone, Default)]
pub struct PendingIds {
    revlog: VecDeque<RevlogId>,
    notes: VecDeque<NoteId>,
    cards: VecDeque<CardId>,
}

impl PendingIds {
    pub fn new(revlog: Vec<RevlogId>, notes: Vec<NoteId>, cards: Vec<CardId>) -> PendingIds {
        PendingIds {
            revlog: revlog.into(),
            notes: notes.into(),
            cards: cards.into(),
        }
    }

    /// The next chunk: one review id, one note id and one card id in turns
    /// (a list skipped once empty) until it holds 250 objects or nothing is
    /// left. The sender sends until it has sent a chunk with `done` true; the
    /// last one is empty when the objects filled the earlier ones exactly.
    pub fn next_chunk(&mut self) -> ChunkIds {
        let mut chunk = ChunkIds::default();
        while chunk.len() < OBJECTS_PER_CHUNK {
            let mut took = false;
            if let Some(id) = self.revlog.pop_front() {
                chunk.revlog.push(id);
                took = true;
            }
            if chunk.len() < OBJECTS_PER_CHUNK
                && let Some(id) = self.notes.pop_front()
            {
                chunk.notes.push(id);
                took = true;
            }
            if chunk.len() < OBJECTS_PER_CHUNK
                && let Some(id) = self.cards.pop_front()
            {
                chunk.cards.push(id);
                took = true;
            }
            if !took {
                break;
            }
        }
        chunk.done = chunk.len() < OBJECTS_PER_CHUNK;
        chunk
    }
}
