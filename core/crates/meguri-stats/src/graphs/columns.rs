//! The column copies of the cards and the review log (`stats/graphs-common`
//! G9.1): one table image per table, each column at the narrowest
//! little-endian width that holds every stored value, the unreadable rows
//! listed apart. An image lives in memory (a request computed from its
//! snapshot) or in a file the derived store maps read-only (G9.3).

use std::collections::HashMap;
use std::sync::Arc;

use meguri_model::card::{CardQueue, CardType};
use meguri_model::card_data::MemoryState;
use meguri_model::revlog::RevlogKind;
use meguri_storage::derived::MappedFile;

use super::rows::{BadCard, BadColumn, BadRow, CardRead, RowRead, StatsCard, StatsRow};

const MAGIC: [u8; 4] = *b"MGSC";
const VERSION: u32 = 1;
const KIND_CARDS: u8 = 1;
const KIND_REVLOG: u8 = 2;

/// The bytes of a table image.
#[derive(Clone, Debug)]
pub(crate) enum Backing {
    Mapped(Arc<MappedFile>),
    Owned(Arc<Vec<u8>>),
}

impl Backing {
    fn bytes(&self) -> &[u8] {
        match self {
            Backing::Mapped(file) => file.bytes(),
            Backing::Owned(bytes) => bytes,
        }
    }
}

/// One column: `rows` values of `width` bytes from `offset`.
#[derive(Clone, Debug)]
struct Column {
    backing: Backing,
    offset: usize,
    width: u8,
}

impl Column {
    fn get(&self, row: usize) -> i64 {
        let bytes = self.backing.bytes();
        let at = self.offset + row * usize::from(self.width);
        match self.width {
            1 => i64::from(bytes[at] as i8),
            2 => i64::from(i16::from_le_bytes([bytes[at], bytes[at + 1]])),
            4 => i64::from(i32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))),
            _ => i64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes")),
        }
    }
}

/// The values of one column being encoded.
trait Values {
    fn count(&self) -> usize;
    fn value(&self, index: usize) -> i64;
}

impl<T: Copy + Into<i64>> Values for Vec<T> {
    fn count(&self) -> usize {
        self.len()
    }

    fn value(&self, index: usize) -> i64 {
        self[index].into()
    }
}

/// The narrowest of 1, 2, 4 and 8 bytes holding every value as a signed
/// integer.
fn width_of(values: &dyn Values) -> u8 {
    let (mut min, mut max) = (0i64, 0i64);
    for index in 0..values.count() {
        let value = values.value(index);
        min = min.min(value);
        max = max.max(value);
    }
    if min >= i64::from(i8::MIN) && max <= i64::from(i8::MAX) {
        1
    } else if min >= i64::from(i16::MIN) && max <= i64::from(i16::MAX) {
        2
    } else if min >= i64::from(i32::MIN) && max <= i64::from(i32::MAX) {
        4
    } else {
        8
    }
}

/// Header, widths, column bytes, extras. Each value list is dropped once
/// written, so the peak is the image and the lists not yet written.
fn image(kind: u8, rows: usize, columns: Vec<Box<dyn Values>>, extras: &[u8]) -> Vec<u8> {
    let widths: Vec<u8> = columns.iter().map(|values| width_of(values.as_ref())).collect();
    let size = 4 + 4 + 1 + 8 + 4 + widths.len() + 8 + widths.iter().map(|w| usize::from(*w) * rows).sum::<usize>() + extras.len();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(kind);
    out.extend_from_slice(&(rows as u64).to_le_bytes());
    out.extend_from_slice(&(widths.len() as u32).to_le_bytes());
    out.extend_from_slice(&widths);
    out.extend_from_slice(&(extras.len() as u64).to_le_bytes());
    for (values, width) in columns.into_iter().zip(&widths) {
        debug_assert_eq!(values.count(), rows);
        for index in 0..values.count() {
            out.extend_from_slice(&values.value(index).to_le_bytes()[..usize::from(*width)]);
        }
    }
    out.extend_from_slice(extras);
    out
}

/// Why an image cannot be read.
#[derive(Debug)]
pub(crate) struct Malformed(pub &'static str);

/// The parts of an image.
struct Parsed {
    rows: usize,
    columns: Vec<Column>,
    extras: std::ops::Range<usize>,
}

fn take<const N: usize>(bytes: &[u8], at: &mut usize) -> Result<[u8; N], Malformed> {
    let slice = bytes.get(*at..*at + N).ok_or(Malformed("truncated"))?;
    *at += N;
    Ok(slice.try_into().expect("N bytes"))
}

fn parse(backing: &Backing, kind: u8, width: usize) -> Result<Parsed, Malformed> {
    let bytes = backing.bytes();
    let mut at = 0;
    if take::<4>(bytes, &mut at)? != MAGIC {
        return Err(Malformed("not a column image"));
    }
    if u32::from_le_bytes(take::<4>(bytes, &mut at)?) != VERSION {
        return Err(Malformed("another image version"));
    }
    if take::<1>(bytes, &mut at)?[0] != kind {
        return Err(Malformed("another table's image"));
    }
    let rows = usize::try_from(u64::from_le_bytes(take::<8>(bytes, &mut at)?)).map_err(|_| Malformed("rows"))?;
    let count = u32::from_le_bytes(take::<4>(bytes, &mut at)?) as usize;
    if count != width {
        return Err(Malformed("another column count"));
    }
    let widths = bytes.get(at..at + count).ok_or(Malformed("truncated"))?.to_vec();
    at += count;
    let extras_len =
        usize::try_from(u64::from_le_bytes(take::<8>(bytes, &mut at)?)).map_err(|_| Malformed("extras"))?;
    let mut columns = Vec::with_capacity(count);
    for width in widths {
        if !matches!(width, 1 | 2 | 4 | 8) {
            return Err(Malformed("a column width"));
        }
        columns.push(Column {
            backing: backing.clone(),
            offset: at,
            width,
        });
        at = rows
            .checked_mul(usize::from(width))
            .and_then(|size| at.checked_add(size))
            .ok_or(Malformed("rows"))?;
    }
    let end = at.checked_add(extras_len).ok_or(Malformed("extras"))?;
    if end != bytes.len() {
        return Err(Malformed("the image's length"));
    }
    Ok(Parsed {
        rows,
        columns,
        extras: at..end,
    })
}

// Card columns, in image order.
const C_ID: usize = 0;
const C_NID: usize = 1;
const C_DID: usize = 2;
const C_ODID: usize = 3;
const C_TYPE: usize = 4;
const C_QUEUE: usize = 5;
const C_DUE: usize = 6;
const C_ODUE: usize = 7;
const C_IVL: usize = 8;
/// Bit 0 memory state, bit 1 decay, bit 2 `lrt`.
const C_PRESENT: usize = 9;
const C_STABILITY: usize = 10;
const C_DIFFICULTY: usize = 11;
const C_DECAY: usize = 12;
const C_LRT: usize = 13;
const CARD_COLUMNS: usize = 14;

const BAD_CARD_BYTES: usize = 8 + 8 + 8 + 1 + 1;

/// The card copy (G9.1): readable cards in ascending id order, then the
/// unreadable ones, also by id. A card's slot is its place in that order.
#[derive(Clone, Debug)]
pub(crate) struct CardTable {
    rows: usize,
    columns: Vec<Column>,
    bad: Vec<BadCard>,
}

fn float(stored: i64) -> f32 {
    f32::from_bits(stored as i32 as u32)
}

impl CardTable {
    pub(crate) fn open(backing: Backing) -> Result<CardTable, Malformed> {
        let parsed = parse(&backing, KIND_CARDS, CARD_COLUMNS)?;
        let extras = &backing.bytes()[parsed.extras];
        if !extras.len().is_multiple_of(BAD_CARD_BYTES) {
            return Err(Malformed("the unreadable cards"));
        }
        let mut bad = Vec::with_capacity(extras.len() / BAD_CARD_BYTES);
        for entry in extras.as_chunks::<BAD_CARD_BYTES>().0 {
            let number = |at: usize| i64::from_le_bytes(entry[at..at + 8].try_into().expect("eight bytes"));
            let present = entry[24];
            bad.push(BadCard {
                id: number(0),
                did: (present & 1 != 0).then(|| number(8)),
                odid: (present & 2 != 0).then(|| number(16)),
                column: BadColumn::from_code(entry[25]).ok_or(Malformed("an unreadable column"))?,
            });
        }
        Ok(CardTable {
            rows: parsed.rows,
            columns: parsed.columns,
            bad,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_cards(cards: &[CardRead]) -> CardTable {
        let mut builder = CardsBuilder::default();
        for card in cards {
            builder.push(*card);
        }
        builder.table()
    }

    /// Readable cards.
    pub(crate) fn readable(&self) -> usize {
        self.rows
    }

    /// Slots: readable cards, then unreadable ones.
    pub(crate) fn slots(&self) -> usize {
        self.rows + self.bad.len()
    }

    pub(crate) fn id(&self, slot: usize) -> i64 {
        if slot < self.rows {
            self.columns[C_ID].get(slot)
        } else {
            self.bad[slot - self.rows].id
        }
    }

    /// The `did` and `odid` of the card in `slot` (G2.1's deck values).
    pub(crate) fn decks(&self, slot: usize) -> (Option<i64>, Option<i64>) {
        if slot < self.rows {
            (Some(self.columns[C_DID].get(slot)), Some(self.columns[C_ODID].get(slot)))
        } else {
            let bad = &self.bad[slot - self.rows];
            (bad.did, bad.odid)
        }
    }

    /// The readable card in `slot` (below [`CardTable::readable`]).
    pub(crate) fn card(&self, slot: usize) -> StatsCard {
        let get = |column: usize| self.columns[column].get(slot);
        let present = get(C_PRESENT);
        StatsCard {
            id: get(C_ID),
            nid: get(C_NID),
            did: get(C_DID),
            odid: get(C_ODID),
            card_type: CardType::from_stored(get(C_TYPE)).unwrap_or(CardType::New),
            queue: CardQueue::from_stored(get(C_QUEUE)).unwrap_or(CardQueue::New),
            due: get(C_DUE) as i32,
            odue: get(C_ODUE) as i32,
            ivl: get(C_IVL) as u32,
            memory: (present & 1 != 0).then(|| MemoryState {
                stability: float(get(C_STABILITY)),
                difficulty: float(get(C_DIFFICULTY)),
            }),
            decay: (present & 2 != 0).then(|| float(get(C_DECAY))),
            lrt: (present & 4 != 0).then(|| get(C_LRT)),
        }
    }

    /// The card in `slot`, readable or not.
    pub(crate) fn read(&self, slot: usize) -> CardRead {
        if slot < self.rows {
            CardRead::Card(self.card(slot))
        } else {
            CardRead::Bad(self.bad[slot - self.rows])
        }
    }

    /// The slot of the card `id`.
    pub(crate) fn slot_of(&self, id: i64) -> Option<usize> {
        let (mut low, mut high) = (0, self.rows);
        while low < high {
            let middle = low + (high - low) / 2;
            match self.columns[C_ID].get(middle).cmp(&id) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        self.bad
            .binary_search_by_key(&id, |bad| bad.id)
            .ok()
            .map(|at| self.rows + at)
    }
}

// Review-log columns, in image order.
const R_ID: usize = 0;
/// The card's slot, −1 for none (a deleted card's row).
const R_SLOT: usize = 1;
const R_EASE: usize = 2;
const R_IVL: usize = 3;
const R_LAST_IVL: usize = 4;
const R_FACTOR: usize = 5;
const R_TIME: usize = 6;
const R_KIND: usize = 7;
const REVLOG_COLUMNS: usize = 8;

const BAD_ROW_BYTES: usize = 8 + 8 + 1 + 1;

/// The review-log copy (G9.1): readable rows in ascending id order with
/// their card's slot in a [`CardTable`], the unreadable ones apart, also by
/// id. Rows with a negative id are never read by a graph (G3) and are not
/// held.
#[derive(Clone, Debug)]
pub(crate) struct RevlogTable {
    rows: usize,
    columns: Vec<Column>,
    bad: Vec<BadRow>,
}

/// A readable row of the copy, with its card's slot.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlottedRow {
    pub row: StatsRow,
    pub slot: Option<usize>,
}

impl RevlogTable {
    pub(crate) fn open(backing: Backing) -> Result<RevlogTable, Malformed> {
        let parsed = parse(&backing, KIND_REVLOG, REVLOG_COLUMNS)?;
        let extras = &backing.bytes()[parsed.extras];
        if !extras.len().is_multiple_of(BAD_ROW_BYTES) {
            return Err(Malformed("the unreadable rows"));
        }
        let mut bad = Vec::with_capacity(extras.len() / BAD_ROW_BYTES);
        for entry in extras.as_chunks::<BAD_ROW_BYTES>().0 {
            let number = |at: usize| i64::from_le_bytes(entry[at..at + 8].try_into().expect("eight bytes"));
            bad.push(BadRow {
                id: number(0),
                cid: (entry[16] & 1 != 0).then(|| number(8)),
                column: BadColumn::from_code(entry[17]).ok_or(Malformed("an unreadable column"))?,
            });
        }
        Ok(RevlogTable {
            rows: parsed.rows,
            columns: parsed.columns,
            bad,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_rows(rows: &[RowRead], slots: &HashMap<i64, usize>) -> RevlogTable {
        let mut builder = RevlogBuilder::default();
        for row in rows {
            builder.push(*row, |cid| slots.get(&cid).copied());
        }
        builder.table()
    }

    pub(crate) fn len(&self) -> usize {
        self.rows
    }

    pub(crate) fn id(&self, index: usize) -> i64 {
        self.columns[R_ID].get(index)
    }

    pub(crate) fn get(&self, index: usize) -> SlottedRow {
        let get = |column: usize| self.columns[column].get(index);
        let slot = get(R_SLOT);
        SlottedRow {
            row: StatsRow {
                id: get(R_ID),
                ease: get(R_EASE) as u8,
                ivl: get(R_IVL) as i32,
                last_ivl: get(R_LAST_IVL) as i32,
                factor: get(R_FACTOR) as u32,
                time: get(R_TIME) as u32,
                kind: RevlogKind::from_stored(get(R_KIND)),
            },
            slot: usize::try_from(slot).ok(),
        }
    }

    /// The first readable row with an id of at least `id`.
    pub(crate) fn lower_bound(&self, id: i64) -> usize {
        let (mut low, mut high) = (0, self.rows);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.id(middle) < id {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    pub(crate) fn bad(&self) -> &[BadRow] {
        &self.bad
    }
}

/// Cards being copied, in ascending id order (G9.1).
#[derive(Default)]
pub(crate) struct CardsBuilder {
    ids: Vec<i64>,
    nids: Vec<i64>,
    dids: Vec<i64>,
    odids: Vec<i64>,
    types: Vec<u8>,
    queues: Vec<i8>,
    dues: Vec<i32>,
    odues: Vec<i32>,
    ivls: Vec<u32>,
    present: Vec<u8>,
    stabilities: Vec<i32>,
    difficulties: Vec<i32>,
    decays: Vec<i32>,
    lrts: Vec<i64>,
    bad: Vec<BadCard>,
}

fn bits32(value: f32) -> i32 {
    value.to_bits() as i32
}

impl CardsBuilder {
    /// Room for `cards` cards, so a build of 150,000 never doubles a list.
    pub(crate) fn with_capacity(cards: usize) -> CardsBuilder {
        CardsBuilder {
            ids: Vec::with_capacity(cards),
            nids: Vec::with_capacity(cards),
            dids: Vec::with_capacity(cards),
            odids: Vec::with_capacity(cards),
            types: Vec::with_capacity(cards),
            queues: Vec::with_capacity(cards),
            dues: Vec::with_capacity(cards),
            odues: Vec::with_capacity(cards),
            ivls: Vec::with_capacity(cards),
            present: Vec::with_capacity(cards),
            stabilities: Vec::with_capacity(cards),
            difficulties: Vec::with_capacity(cards),
            decays: Vec::with_capacity(cards),
            lrts: Vec::with_capacity(cards),
            bad: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, card: CardRead) {
        let card = match card {
            CardRead::Card(card) => card,
            CardRead::Bad(bad) => {
                self.bad.push(bad);
                return;
            }
        };
        self.ids.push(card.id);
        self.nids.push(card.nid);
        self.dids.push(card.did);
        self.odids.push(card.odid);
        self.types.push(card.card_type.stored() as u8);
        self.queues.push(card.queue.stored() as i8);
        self.dues.push(card.due);
        self.odues.push(card.odue);
        self.ivls.push(card.ivl);
        self.present
            .push(u8::from(card.memory.is_some()) | u8::from(card.decay.is_some()) << 1 | u8::from(card.lrt.is_some()) << 2);
        self.stabilities.push(card.memory.map_or(0, |m| bits32(m.stability)));
        self.difficulties.push(card.memory.map_or(0, |m| bits32(m.difficulty)));
        self.decays.push(card.decay.map_or(0, bits32));
        self.lrts.push(card.lrt.unwrap_or(0));
    }

    /// The slot each card id takes: readable cards first, then the others.
    pub(crate) fn slots(&self) -> HashMap<i64, usize> {
        let mut slots: HashMap<i64, usize> = HashMap::with_capacity(self.ids.len() + self.bad.len());
        for (slot, id) in self.ids.iter().enumerate() {
            slots.insert(*id, slot);
        }
        for (at, bad) in self.bad.iter().enumerate() {
            slots.insert(bad.id, self.ids.len() + at);
        }
        slots
    }

    pub(crate) fn image(self) -> Vec<u8> {
        let mut extras = Vec::with_capacity(self.bad.len() * BAD_CARD_BYTES);
        for bad in &self.bad {
            extras.extend_from_slice(&bad.id.to_le_bytes());
            extras.extend_from_slice(&bad.did.unwrap_or(0).to_le_bytes());
            extras.extend_from_slice(&bad.odid.unwrap_or(0).to_le_bytes());
            extras.push(u8::from(bad.did.is_some()) | u8::from(bad.odid.is_some()) << 1);
            extras.push(bad.column.code());
        }
        let rows = self.ids.len();
        let columns: Vec<Box<dyn Values>> = vec![
            Box::new(self.ids),
            Box::new(self.nids),
            Box::new(self.dids),
            Box::new(self.odids),
            Box::new(self.types),
            Box::new(self.queues),
            Box::new(self.dues),
            Box::new(self.odues),
            Box::new(self.ivls),
            Box::new(self.present),
            Box::new(self.stabilities),
            Box::new(self.difficulties),
            Box::new(self.decays),
            Box::new(self.lrts),
        ];
        image(KIND_CARDS, rows, columns, &extras)
    }

    pub(crate) fn table(self) -> CardTable {
        CardTable::open(Backing::Owned(Arc::new(self.image()))).expect("an image this code wrote")
    }
}

/// Review-log rows being copied, in ascending id order (G9.1).
#[derive(Default)]
pub(crate) struct RevlogBuilder {
    ids: Vec<i64>,
    slots: Vec<i32>,
    eases: Vec<u8>,
    ivls: Vec<i32>,
    last_ivls: Vec<i32>,
    factors: Vec<u32>,
    times: Vec<u32>,
    kinds: Vec<u8>,
    bad: Vec<BadRow>,
}

impl RevlogBuilder {
    /// Room for `rows` rows, so a build of 1,200,000 never doubles a list.
    pub(crate) fn with_capacity(rows: usize) -> RevlogBuilder {
        RevlogBuilder {
            ids: Vec::with_capacity(rows),
            slots: Vec::with_capacity(rows),
            eases: Vec::with_capacity(rows),
            ivls: Vec::with_capacity(rows),
            last_ivls: Vec::with_capacity(rows),
            factors: Vec::with_capacity(rows),
            times: Vec::with_capacity(rows),
            kinds: Vec::with_capacity(rows),
            bad: Vec::new(),
        }
    }

    /// Adds a row; `slot` gives its card's slot, none for a deleted card.
    pub(crate) fn push(&mut self, row: RowRead, slot: impl Fn(i64) -> Option<usize>) {
        match row {
            RowRead::Row { row, cid } => self.push_slotted(row, slot(cid)),
            RowRead::Bad(bad) => self.bad.push(bad),
        }
    }

    /// Adds a readable row with its card's slot, none for a deleted card.
    pub(crate) fn push_slotted(&mut self, row: StatsRow, slot: Option<usize>) {
        self.ids.push(row.id);
        self.slots.push(slot.and_then(|slot| i32::try_from(slot).ok()).unwrap_or(-1));
        self.eases.push(row.ease);
        self.ivls.push(row.ivl);
        self.last_ivls.push(row.last_ivl);
        self.factors.push(row.factor);
        self.times.push(row.time);
        self.kinds.push(row.kind.stored() as u8);
    }

    pub(crate) fn image(self) -> Vec<u8> {
        let mut extras = Vec::with_capacity(self.bad.len() * BAD_ROW_BYTES);
        for bad in &self.bad {
            extras.extend_from_slice(&bad.id.to_le_bytes());
            extras.extend_from_slice(&bad.cid.unwrap_or(0).to_le_bytes());
            extras.push(u8::from(bad.cid.is_some()));
            extras.push(bad.column.code());
        }
        let rows = self.ids.len();
        let columns: Vec<Box<dyn Values>> = vec![
            Box::new(self.ids),
            Box::new(self.slots),
            Box::new(self.eases),
            Box::new(self.ivls),
            Box::new(self.last_ivls),
            Box::new(self.factors),
            Box::new(self.times),
            Box::new(self.kinds),
        ];
        image(KIND_REVLOG, rows, columns, &extras)
    }

    pub(crate) fn table(self) -> RevlogTable {
        RevlogTable::open(Backing::Owned(Arc::new(self.image()))).expect("an image this code wrote")
    }
}

/// A 64-bit FNV-1a checksum of a file's bytes (G9.3's record rows).
pub(crate) fn checksum(bytes: &[u8]) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_are_the_narrowest_holding_every_value() {
        let width = |values: &[i64]| width_of(&values.to_vec());
        assert_eq!(width(&[]), 1);
        assert_eq!(width(&[-128, 127]), 1);
        assert_eq!(width(&[128]), 2);
        assert_eq!(width(&[-32_769]), 4);
        assert_eq!(width(&[i64::from(i32::MAX)]), 4);
        assert_eq!(width(&[i64::from(i32::MAX) + 1]), 8);
        assert_eq!(width(&[i64::MIN]), 8);
    }

    #[test]
    fn a_card_image_reads_back_every_value() {
        let card = StatsCard {
            id: 1_700_000_000_000,
            nid: 5,
            did: -7,
            odid: 0,
            card_type: CardType::Relearning,
            queue: CardQueue::BuriedByUser,
            due: i32::MIN,
            odue: i32::MAX,
            ivl: u32::MAX,
            memory: Some(MemoryState {
                stability: -0.0,
                difficulty: f32::MAX,
            }),
            decay: None,
            lrt: Some(i64::MIN),
        };
        let bad = BadCard {
            id: 1_700_000_000_001,
            did: None,
            odid: Some(3),
            column: BadColumn::CardQueue,
        };
        let table = CardTable::from_cards(&[CardRead::Card(card), CardRead::Bad(bad)]);
        assert_eq!(table.readable(), 1);
        assert_eq!(table.slots(), 2);
        let back = table.card(0);
        assert_eq!(back.memory.unwrap().stability.to_bits(), (-0.0f32).to_bits());
        assert_eq!(back, card);
        assert_eq!(table.read(1), CardRead::Bad(bad));
        assert_eq!(table.slot_of(card.id), Some(0));
        assert_eq!(table.slot_of(bad.id), Some(1));
        assert_eq!(table.slot_of(9), None);
    }

    #[test]
    fn a_revlog_image_reads_back_every_value_and_slot() {
        let row = |id, kind| StatsRow {
            id,
            ease: 255,
            ivl: i32::MIN,
            last_ivl: -86_400,
            factor: u32::MAX,
            time: 0,
            kind,
        };
        let rows = [
            RowRead::Row {
                row: row(10, RevlogKind::Manual),
                cid: 1,
            },
            RowRead::Bad(BadRow {
                id: 11,
                cid: None,
                column: BadColumn::RevlogEase,
            }),
            RowRead::Row {
                row: row(12, RevlogKind::Rescheduled),
                cid: 2,
            },
        ];
        let slots = HashMap::from([(1, 0)]);
        let table = RevlogTable::from_rows(&rows, &slots);
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(0).row, row(10, RevlogKind::Manual));
        assert_eq!(table.get(0).slot, Some(0));
        assert_eq!(table.get(1).slot, None);
        assert_eq!(table.bad().len(), 1);
        assert_eq!(table.lower_bound(11), 1);
        assert_eq!(table.lower_bound(13), 2);
    }

    #[test]
    fn a_damaged_image_is_refused() {
        let image = CardsBuilder::default().image();
        let short = Backing::Owned(Arc::new(image[..image.len() - 1].to_vec()));
        assert!(CardTable::open(short).is_err());
        let revlog = Backing::Owned(Arc::new(image.clone()));
        assert!(RevlogTable::open(revlog).is_err());
        assert!(CardTable::open(Backing::Owned(Arc::new(image))).is_ok());
    }
}
