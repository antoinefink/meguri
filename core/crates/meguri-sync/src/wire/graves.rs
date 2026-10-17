//! `start` and `applyGraves` (specs/sync/wire-format.md W4, W5; R1521,
//! R1522): the start request, the graves object both directions use, and the
//! split of a sync's graves into requests of at most 250 ids.

use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::usn::Usn;

use super::read::{self, At};
use super::write;
use crate::error::JobError;
use crate::transport::Method;

/// The most ids one `applyGraves` request holds, all three lists together
/// (W5.2).
pub const GRAVES_PER_REQUEST: usize = 250;

/// The `start` request (W4): `{"minUsn":…,"lnewer":…,"graves":null}`.
/// `min_usn` is the local `col.usn`; `local_is_newer` is true when the
/// local `col.mod` is greater than the server's.
pub fn encode_start(min_usn: Usn, local_is_newer: bool) -> Vec<u8> {
    let mut out = String::from("{\"minUsn\":");
    write::int(&mut out, min_usn.0);
    out.push_str(",\"lnewer\":");
    write::boolean(&mut out, local_is_newer);
    out.push_str(",\"graves\":null}");
    out.into_bytes()
}

/// Deleted cards, notes and decks (W5.1). Order inside a list means nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graves {
    pub cards: Vec<CardId>,
    pub notes: Vec<NoteId>,
    pub decks: Vec<DeckId>,
}

impl Graves {
    pub fn len(&self) -> usize {
        self.cards.len() + self.notes.len() + self.decks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The `applyGraves` chunks of a sync (W5.2): each at most
    /// [`GRAVES_PER_REQUEST`] ids, taken from the card graves first, then the
    /// note graves, then the deck graves. None when there is nothing to send.
    pub fn into_chunks(self) -> Vec<Graves> {
        let mut chunks = Vec::with_capacity(self.len().div_ceil(GRAVES_PER_REQUEST));
        let mut current = Graves::default();
        let mut cards = self.cards.into_iter();
        let mut notes = self.notes.into_iter();
        let mut decks = self.decks.into_iter();
        loop {
            if current.len() == GRAVES_PER_REQUEST {
                chunks.push(std::mem::take(&mut current));
            }
            if let Some(card) = cards.next() {
                current.cards.push(card);
            } else if let Some(note) = notes.next() {
                current.notes.push(note);
            } else if let Some(deck) = decks.next() {
                current.decks.push(deck);
            } else {
                break;
            }
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }

    /// The graves object: `{"cards":[…],"decks":[…],"notes":[…]}`.
    fn write(&self, out: &mut String) {
        let cards: Vec<i64> = self.cards.iter().map(|id| id.0).collect();
        let decks: Vec<i64> = self.decks.iter().map(|id| id.0).collect();
        let notes: Vec<i64> = self.notes.iter().map(|id| id.0).collect();
        out.push_str("{\"cards\":");
        write::ints(out, &cards);
        out.push_str(",\"decks\":");
        write::ints(out, &decks);
        out.push_str(",\"notes\":");
        write::ints(out, &notes);
        out.push('}');
    }
}

/// One `applyGraves` request: `{"chunk":<graves object>}`. The caller splits
/// with [`Graves::into_chunks`].
pub fn encode_apply_graves(chunk: &Graves) -> Vec<u8> {
    let mut out = String::from("{\"chunk\":");
    chunk.write(&mut out);
    out.push('}');
    out.into_bytes()
}

/// The `start` answer (W4): the deletions made on the server since
/// `minUsn`.
pub fn decode_graves(body: &[u8]) -> Result<Graves, JobError> {
    decode(body).map_err(|err| JobError::bad_response(Method::Start, err.to_string()))
}

fn decode(body: &[u8]) -> read::Result<Graves> {
    let at = At::Root;
    let root = read::object(read::body(body)?, at)?;
    let ids = |key: &str| -> read::Result<Vec<i64>> {
        let list = root.required(key, at)?;
        let at = at.member(key);
        read::array(list, at)?
            .into_iter()
            .enumerate()
            .map(|(index, raw)| read::strict::<i64>(raw, at.index(index)))
            .collect()
    };
    Ok(Graves {
        cards: ids("cards")?.into_iter().map(CardId).collect(),
        notes: ids("notes")?.into_iter().map(NoteId).collect(),
        decks: ids("decks")?.into_iter().map(DeckId).collect(),
    })
}
