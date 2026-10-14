//! Deck blobs: deck common and deck kind (`specs/model/protobuf-records.md`
//! P6.4, P6.5).

use super::{Decode, Encode, Mode, Reader, Result, UnknownFields, WireType, Writer, blob_message, encode_with};
use crate::error::{BlobColumn, BlobError, ModelError};
use crate::proto::BlobMessage;

/// P6.4, `decks.common`. Numbers 8 to 13 are reserved by Anki and kept as
/// unknown fields.
#[derive(Clone, Debug, Default)]
pub struct DeckCommon {
    pub study_collapsed: bool,
    pub browser_collapsed: bool,
    /// The day number the counters below belong to.
    pub last_day_studied: u32,
    pub new_studied: i32,
    pub review_studied: i32,
    /// The old v1 scheduler's learning count; kept, not used.
    pub learning_studied: i32,
    pub milliseconds_studied: i32,
    pub(crate) other: Vec<u8>,
    pub(crate) unknown: UnknownFields,
}

impl DeckCommon {
    /// Legacy JSON keys (P7), opaque bytes.
    pub fn other(&self) -> &[u8] {
        &self.other
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for DeckCommon {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.study_collapsed = r.bool(wire)?,
            2 => self.browser_collapsed = r.bool(wire)?,
            3 => self.last_day_studied = r.uint32(wire)?,
            4 => self.new_studied = r.int32(wire)?,
            5 => self.review_studied = r.int32(wire)?,
            6 => self.learning_studied = r.int32(wire)?,
            7 => self.milliseconds_studied = r.int32(wire)?,
            255 => self.other = r.bytes(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for DeckCommon {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.bool(1, self.study_collapsed);
        w.bool(2, self.browser_collapsed);
        w.uint32(3, self.last_day_studied);
        w.int32(4, self.new_studied);
        w.int32(5, self.review_studied);
        w.int32(6, self.learning_studied);
        w.int32(7, self.milliseconds_studied);
        w.bytes(255, &self.other);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

blob_message!(DeckCommon, BlobColumn::DeckCommon);

/// The member of the deck kind one-of (P6.5).
#[derive(Clone, Debug)]
pub enum DeckKind {
    Normal(NormalDeck),
    /// Meguri never creates filtered decks; it keeps them intact (R1483).
    Filtered(FilteredDeck),
}

/// P6.5, `decks.kind`: exactly one member.
#[derive(Clone, Debug)]
pub struct DeckKindMessage {
    pub kind: DeckKind,
    pub(crate) unknown: UnknownFields,
}

impl DeckKindMessage {
    pub fn new(kind: DeckKind) -> DeckKindMessage {
        DeckKindMessage {
            kind,
            unknown: UnknownFields::default(),
        }
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

/// The deck kind while it is read: no member until one occurs.
#[derive(Default)]
struct DeckKindRead {
    kind: Option<DeckKind>,
    unknown: UnknownFields,
}

impl Decode for DeckKindRead {
    /// A member replaces the other one; a second occurrence of the same
    /// member merges into it (P2).
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match (field, &mut self.kind) {
            (1, Some(DeckKind::Normal(normal))) => r.message(wire, normal)?,
            (1, _) => {
                let mut normal = NormalDeck::default();
                r.message(wire, &mut normal)?;
                self.kind = Some(DeckKind::Normal(normal));
            }
            (2, Some(DeckKind::Filtered(filtered))) => r.message(wire, filtered)?,
            (2, _) => {
                let mut filtered = FilteredDeck::default();
                r.message(wire, &mut filtered)?;
                self.kind = Some(DeckKind::Filtered(filtered));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for DeckKindMessage {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        match &self.kind {
            DeckKind::Normal(normal) => w.message(1, normal),
            DeckKind::Filtered(filtered) => w.message(2, filtered),
        }
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl BlobMessage for DeckKindMessage {
    const COLUMN: BlobColumn = BlobColumn::DeckKind;

    fn decode(bytes: &[u8]) -> std::result::Result<DeckKindMessage, ModelError> {
        let error = |reason| ModelError::Blob {
            column: BlobColumn::DeckKind,
            reason,
        };
        let read: DeckKindRead = super::decode_message(bytes).map_err(error)?;
        let kind = read.kind.ok_or(error(BlobError::NoDeckKind))?;
        Ok(DeckKindMessage {
            kind,
            unknown: read.unknown,
        })
    }

    fn encode(&self) -> Vec<u8> {
        encode_with(self, Mode::Canonical)
    }

    fn same_content(&self, other: &DeckKindMessage) -> bool {
        encode_with(self, Mode::Exact) == encode_with(other, Mode::Exact)
    }

    fn equivalent(&self, other: &DeckKindMessage) -> bool {
        encode_with(self, Mode::Normalized) == encode_with(other, Mode::Normalized)
    }
}

/// P6.5.1. It has no `other`; numbers 11 to 15 are unknown fields.
#[derive(Clone, Debug, Default)]
pub struct NormalDeck {
    /// The deck's preset (`deck_config.id`).
    pub config_id: i64,
    /// Legacy custom-study extension; kept.
    pub extend_new: u32,
    /// Legacy custom-study extension; kept.
    pub extend_review: u32,
    pub description: String,
    pub markdown_description: bool,
    pub review_limit: Option<u32>,
    pub new_limit: Option<u32>,
    pub review_limit_today: Option<DayLimit>,
    pub new_limit_today: Option<DayLimit>,
    pub desired_retention: Option<f32>,
    pub(crate) unknown: UnknownFields,
}

impl NormalDeck {
    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for NormalDeck {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.config_id = r.int64(wire)?,
            2 => self.extend_new = r.uint32(wire)?,
            3 => self.extend_review = r.uint32(wire)?,
            4 => self.description = r.string(wire)?,
            5 => self.markdown_description = r.bool(wire)?,
            6 => self.review_limit = Some(r.uint32(wire)?),
            7 => self.new_limit = Some(r.uint32(wire)?),
            8 => r.message(wire, self.review_limit_today.get_or_insert_with(DayLimit::default))?,
            9 => r.message(wire, self.new_limit_today.get_or_insert_with(DayLimit::default))?,
            10 => self.desired_retention = Some(r.float(wire)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for NormalDeck {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.int64(1, self.config_id);
        w.uint32(2, self.extend_new);
        w.uint32(3, self.extend_review);
        w.string(4, &self.description);
        w.bool(5, self.markdown_description);
        w.opt_uint32(6, self.review_limit);
        w.opt_uint32(7, self.new_limit);
        if let Some(limit) = &self.review_limit_today {
            w.message(8, limit);
        }
        if let Some(limit) = &self.new_limit_today {
            w.message(9, limit);
        }
        w.opt_float(10, self.desired_retention);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

/// P6.5.2, a today-only limit.
#[derive(Clone, Debug, Default)]
pub struct DayLimit {
    pub limit: u32,
    /// The day number it applies to.
    pub today: u32,
    pub(crate) unknown: UnknownFields,
}

impl DayLimit {
    pub fn new(limit: u32, today: u32) -> DayLimit {
        DayLimit {
            limit,
            today,
            unknown: UnknownFields::default(),
        }
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for DayLimit {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.limit = r.uint32(wire)?,
            2 => self.today = r.uint32(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for DayLimit {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.uint32(1, self.limit);
        w.uint32(2, self.today);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

/// P6.5.3.
#[derive(Clone, Debug, Default)]
pub struct FilteredDeck {
    pub reschedule: bool,
    /// One or two searches.
    pub search_terms: Vec<SearchTerm>,
    /// Old-scheduler delays; kept.
    pub delays: Vec<f32>,
    /// Older preview delay; kept.
    pub preview_delay: u32,
    pub preview_hard_secs: u32,
    pub preview_good_secs: u32,
    pub preview_again_secs: u32,
    pub(crate) unknown: UnknownFields,
}

impl FilteredDeck {
    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for FilteredDeck {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.reschedule = r.bool(wire)?,
            2 => {
                let mut term = SearchTerm::default();
                r.message(wire, &mut term)?;
                self.search_terms.push(term);
            }
            3 => r.floats(wire, &mut self.delays)?,
            4 => self.preview_delay = r.uint32(wire)?,
            5 => self.preview_hard_secs = r.uint32(wire)?,
            6 => self.preview_good_secs = r.uint32(wire)?,
            7 => self.preview_again_secs = r.uint32(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for FilteredDeck {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.bool(1, self.reschedule);
        for term in &self.search_terms {
            w.message(2, term);
        }
        w.floats(3, &self.delays);
        w.uint32(4, self.preview_delay);
        w.uint32(5, self.preview_hard_secs);
        w.uint32(6, self.preview_good_secs);
        w.uint32(7, self.preview_again_secs);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

/// P6.5.4.
#[derive(Clone, Debug, Default)]
pub struct SearchTerm {
    pub search: String,
    pub limit: u32,
    /// 0 oldest reviewed first, 1 random, 2 intervals ascending, 3 intervals
    /// descending, 4 lapses, 5 added, 6 due, 7 reverse added, 8 retrievability
    /// ascending, 9 retrievability descending, 10 relative overdueness.
    pub order: i32,
    pub(crate) unknown: UnknownFields,
}

impl SearchTerm {
    pub fn new(search: String, limit: u32, order: i32) -> SearchTerm {
        SearchTerm {
            search,
            limit,
            order,
            unknown: UnknownFields::default(),
        }
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for SearchTerm {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.search = r.string(wire)?,
            2 => self.limit = r.uint32(wire)?,
            3 => self.order = r.int32(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for SearchTerm {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.string(1, &self.search);
        w.uint32(2, self.limit);
        w.int32(3, self.order);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}
