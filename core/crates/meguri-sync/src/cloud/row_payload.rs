//! K6.1 projections of raw stored rows. These functions never write a collection.
use super::{Error, entity_codec as c};
use c::wire::Message;
use meguri_model::ids::{CardId, NotetypeId};
use meguri_storage::{Cell, ReadRows, RowImage, RowKey, Table};

/// Only raw R1–R10 failures and the canonical K7 cap are entity flags.
/// Provider, layout, SQL and identity failures retain their original errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum RowFailure {
    Oversize,
    Unreadable { table: &'static str, column: &'static str },
}
impl<'de> serde::Deserialize<'de> for RowFailure {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        enum Value { Oversize, Unreadable { table: String, column: String } }
        Ok(match <Value as serde::Deserialize>::deserialize(deserializer)? {
            Value::Oversize => Self::Oversize,
            Value::Unreadable { table, column } => {
                let t = Table::ALL.into_iter().find(|t| t.name() == table)
                    .ok_or_else(|| serde::de::Error::custom("invalid raw row table"))?;
                let column = t.columns().iter().copied().find(|c| *c == column)
                    .ok_or_else(|| serde::de::Error::custom("invalid raw row column"))?;
                Self::Unreadable { table: t.name(), column }
            }
        })
    }
}
pub(super) fn codec_error(error: c::CodecError) -> Error {
    match error {
        c::CodecError::Oversize => Error::RowFailure(RowFailure::Oversize),
        _ => Error::InvalidData,
    }
}

/// Validate storage classes/ranges without decoding TEXT into Rust strings or
/// replacing tolerant values. Payload projection below always uses raw cells.
pub(super) fn readable(t: Table, r: &[Cell]) -> Result<(), Error> {
    use meguri_model::proto::{BlobMessage, deck::*, deck_config::*, notetype::*};
    let bad = |i: usize| Error::RowFailure(RowFailure::Unreadable {
        table: t.name(), column: t.columns()[i],
    });
    if r.len() != t.columns().len() { return Err(Error::InvalidData); }
    let integer = |i: usize, lo: i64, hi: i64| match r.get(i) {
        Some(Cell::Integer(v)) if (lo..=hi).contains(v) => Ok(()),
        _ => Err(bad(i)),
    };
    let text = |i: usize| if matches!(r[i], Cell::Text(_)) { Ok(()) } else { Err(bad(i)) };
    let blob = |i: usize| match &r[i] { Cell::Blob(v) => Ok(v.as_slice()), _ => Err(bad(i)) };
    let i64s = |indices: &[usize]| -> Result<(), Error> {
        for i in indices { integer(*i, i64::MIN, i64::MAX)?; } Ok(())
    };
    if let Some(i) = t.usn_column() { integer(i, i32::MIN.into(), i32::MAX.into())?; }
    match t {
        Table::Notes => {
            i64s(&[0, 2, 3, 8, 9])?;
            for i in [1, 5, 6, 10] { text(i)?; }
            if !matches!(r[7], Cell::Text(_) | Cell::Integer(_) | Cell::Real(_)) { return Err(bad(7)); }
        }
        Table::Cards => {
            if let Err(error) = meguri_storage::rows::cards::validated_note_id(r) {
                return Err(Error::RowFailure(RowFailure::Unreadable {
                    table: error.table,
                    column: error.column,
                }));
            }
        }
        Table::Revlog => {
            i64s(&[0, 1, 8])?;
            integer(3, 0, u8::MAX.into())?;
            integer(6, 0, u32::MAX.into())?;
            for i in [4, 5] {
                match &r[i] {
                    Cell::Real(v) if v.is_finite() && v.trunc() >= i32::MIN as f64 && v.trunc() <= i32::MAX as f64 => (),
                    _ => integer(i, i32::MIN.into(), i32::MAX.into())?,
                }
            }
        }
        Table::Notetypes | Table::Decks | Table::DeckConfig => {
            i64s(&[0, 2])?; text(1)?;
            match t {
                Table::Notetypes => { NotetypeConfig::decode(blob(4)?).map_err(|_| bad(4))?; }
                Table::Decks => {
                    DeckCommon::decode(blob(4)?).map_err(|_| bad(4))?;
                    DeckKindMessage::decode(blob(5)?).map_err(|_| bad(5))?;
                }
                _ => { DeckConfigMessage::decode(blob(4)?).map_err(|_| bad(4))?; }
            }
        }
        Table::Fields | Table::Templates => {
            i64s(&[0])?; integer(1, 0, u32::MAX.into())?; text(2)?;
            if t == Table::Fields { FieldConfig::decode(blob(3)?).map_err(|_| bad(3))?; }
            else { i64s(&[3])?; integer(4, i32::MIN.into(), i32::MAX.into())?; TemplateConfig::decode(blob(5)?).map_err(|_| bad(5))?; }
        }
        Table::Tags => { text(0)?; integer(2, 0, 1)?; if !matches!(r[3], Cell::Null) { blob(3)?; } }
        // R9's non-BLOB value is tolerant, not an unreadable row. The wire
        // format has no Stored alternative here, so projection still refuses it.
        Table::Config => { text(0)?; i64s(&[2])?; }
        _ => return Err(Error::InvalidData),
    }
    Ok(())
}

pub const KINDS: [c::Kind; 9] = [
    c::Kind::Note,
    c::Kind::Card,
    c::Kind::Revlog,
    c::Kind::Deck,
    c::Kind::Preset,
    c::Kind::Notetype,
    c::Kind::Tag,
    c::Kind::Config,
    c::Kind::MediaName,
];
pub fn table(kind: c::Kind) -> Option<Table> {
    Some(match kind {
        c::Kind::Note => Table::Notes,
        c::Kind::Card => Table::Cards,
        c::Kind::Revlog => Table::Revlog,
        c::Kind::Deck => Table::Decks,
        c::Kind::Preset => Table::DeckConfig,
        c::Kind::Notetype => Table::Notetypes,
        c::Kind::Tag => Table::Tags,
        c::Kind::Config => Table::Config,
        c::Kind::MediaName => return None,
    })
}
pub trait View {
    fn row(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, Error>;
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, Error>;
    fn parts(&self, table: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error>;
    fn raw_parts(&self, table: Table, id: i64, after: Option<&Cell>) -> Result<Vec<RowImage>, Error> {
        let ord = after.map(|c| int(std::slice::from_ref(c), 0)).transpose()?;
        self.parts(table, id, ord)
    }
    fn history(&self, id: i64) -> Result<i64, Error>;
}
pub struct ReadView<'a, T: ReadRows + ?Sized>(pub &'a T);
impl<T: ReadRows + ?Sized> View for ReadView<'_, T> {
    fn row(&self, t: Table, k: &RowKey) -> Result<Option<RowImage>, Error> {
        Ok(self.0.row_image(t, k)?)
    }
    fn page(&self, t: Table, k: Option<&RowKey>) -> Result<Vec<RowImage>, Error> {
        Ok(self.0.row_page(t, k)?)
    }
    fn parts(&self, t: Table, id: i64, k: Option<i64>) -> Result<Vec<RowImage>, Error> {
        Ok(match t {
            Table::Fields => self.0.notetype_fields_page(NotetypeId(id), k)?,
            Table::Templates => self.0.notetype_templates_page(NotetypeId(id), k)?,
            _ => return Err(Error::InvalidData),
        })
    }
    fn raw_parts(&self, table: Table, id: i64, after: Option<&Cell>) -> Result<Vec<RowImage>, Error> {
        Ok(self.0.notetype_parts_raw_page(table, NotetypeId(id), after)?)
    }
    fn history(&self, id: i64) -> Result<i64, Error> {
        Ok(self.0.max_revlog_id(CardId(id))?)
    }
}
pub fn int(r: &[Cell], i: usize) -> Result<i64, Error> {
    match r.get(i) {
        Some(Cell::Integer(v)) => Ok(*v),
        _ => Err(Error::InvalidData),
    }
}
fn bytes(r: &[Cell], i: usize, blob: bool) -> Result<Vec<u8>, Error> {
    match r.get(i) {
        Some(Cell::Blob(v)) if blob => Ok(v.clone()),
        Some(Cell::Text(v)) if !blob => Ok(v.clone()),
        _ => Err(Error::InvalidData),
    }
}
fn stored(r: &[Cell], i: usize) -> Result<c::Stored, Error> {
    Ok(match r.get(i) {
        Some(Cell::Integer(v)) => c::Stored::Integer(*v),
        Some(Cell::Real(v)) => c::Stored::Real(*v),
        Some(Cell::Text(v)) => c::Stored::Text(v.clone()),
        Some(Cell::Blob(v)) => c::Stored::Blob(v.clone()),
        _ => return Err(Error::InvalidData),
    })
}
pub fn key(kind: c::Kind, row: &[Cell]) -> Result<Vec<u8>, Error> {
    if (kind as u8) <= 6 {
        return Ok(c::id_key(int(row, 0)?).to_vec());
    }
    let b = bytes(row, 0, false)?;
    let text = std::str::from_utf8(&b).map_err(|_| Error::InvalidData)?;
    Ok(if kind == c::Kind::Tag {
        meguri_text::fold::folded(text).into_bytes()
    } else {
        b
    })
}
pub(super) fn tracking_key(kind: c::Kind, key: &RowKey) -> Result<Vec<u8>, Error> {
    match (kind, key) {
        (
            c::Kind::Note | c::Kind::Card | c::Kind::Revlog | c::Kind::Deck | c::Kind::Preset | c::Kind::Notetype,
            RowKey::Id(id),
        ) => Ok(c::id_key(*id).to_vec()),
        (c::Kind::Tag | c::Kind::Config, RowKey::Text(bytes)) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidData)?;
            Ok(if kind == c::Kind::Tag {
                meguri_text::fold::folded(text).into_bytes()
            } else {
                bytes.clone()
            })
        }
        _ => Err(Error::InvalidData),
    }
}
pub fn notetype(view: &impl View, row: &[Cell], generation: u64) -> Result<c::Notetype, Error> {
    if generation == 0 {
        return Err(Error::InvalidData);
    }
    readable(Table::Notetypes, row)?;
    let id = int(row, 0)?;
    let mut n = c::Notetype {
        id,
        name: bytes(row, 1, false)?,
        mtime_secs: int(row, 2)?,
        config: bytes(row, 4, true)?,
        fields: Vec::new(),
        templates: Vec::new(),
        struct_gen: generation,
    };
    for t in [Table::Fields, Table::Templates] {
        let mut after = None;
        loop {
            let page = view.parts(t, id, after)?;
            if page.is_empty() {
                break;
            }
            for p in page {
                readable(t, &p)?;
                let ord = int(&p, 1)?;
                if int(&p, 0)? != id { return Err(Error::InvalidData); }
                if ord != after.map_or(0, |a| a + 1) {
                    return Err(Error::RowFailure(RowFailure::Unreadable { table: t.name(), column: "ord" }));
                }
                let name = bytes(&p, 2, false)?;
                let config = bytes(&p, if t == Table::Fields { 3 } else { 5 }, true)?;
                if t == Table::Fields {
                    n.fields.push(c::Field { ord, name, config });
                } else {
                    n.templates.push(c::Template {
                        ord,
                        name,
                        mtime_secs: int(&p, 3)?,
                        config,
                    });
                }
                after = Some(ord);
                n.encoded_len(false).map_err(codec_error)?;
            }
        }
    }
    n.encoded_len(false).map_err(codec_error)?;
    Ok(n)
}
/// An acknowledged generation is usable only when the complete raw layout agrees.
pub fn layout(view: &impl View, id: i64, acked: &c::Notetype) -> Result<u64, Error> {
    let row = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
    let actual = match notetype(view, &row, acked.struct_gen) {
        Ok(actual) => actual,
        // A dependency's bad row does not make this row unreadable. Retain
        // the acknowledged layout for projection; the send reference guard
        // holds every dependent until that notetype's own flag is repaired.
        Err(Error::RowFailure(_)) => return Ok(acked.struct_gen),
        Err(other) => return Err(other),
    };
    if actual != *acked {
        return Err(Error::InvalidData);
    }
    Ok(acked.struct_gen)
}
/// Compatibility of two admitted wire images under the server's counter.
/// This is not proof of local ordinal origins, including when optional IDs match.
/// Local projection requires a retained image or explicit native identity maps.
pub(super) fn same_layout(a: &c::Notetype, b: &c::Notetype) -> Result<bool, Error> {
    use meguri_model::proto::{BlobMessage, notetype::*};
    let ac = NotetypeConfig::decode(&a.config).map_err(|_| Error::InvalidData)?;
    let bc = NotetypeConfig::decode(&b.config).map_err(|_| Error::InvalidData)?;
    if a.id != b.id || a.struct_gen == 0 || a.struct_gen != b.struct_gen
        || (ac.kind, ac.sort_field_idx) != (bc.kind, bc.sort_field_idx)
        || a.fields.len() != b.fields.len() || a.templates.len() != b.templates.len()
    { return Ok(false); }
    let mut field_ids = (Vec::new(), Vec::new());
    for (a, b) in a.fields.iter().zip(&b.fields) {
        let ac = FieldConfig::decode(&a.config).map_err(|_| Error::InvalidData)?;
        let bc = FieldConfig::decode(&b.config).map_err(|_| Error::InvalidData)?;
        if a.ord != b.ord { return Ok(false); }
        field_ids.0.push(ac.id);
        field_ids.1.push(bc.id);
    }
    let mut template_ids = (Vec::new(), Vec::new());
    for (a, b) in a.templates.iter().zip(&b.templates) {
        let ac = TemplateConfig::decode(&a.config).map_err(|_| Error::InvalidData)?;
        let bc = TemplateConfig::decode(&b.config).map_err(|_| Error::InvalidData)?;
        if a.ord != b.ord { return Ok(false); }
        template_ids.0.push(ac.id);
        template_ids.1.push(bc.id);
    }
    // IDs live inside whole config parts. A changed ID alone is not a
    // structural edit; a permutation of retained identities is a reorder.
    for (left, right) in [field_ids, template_ids] {
        if left != right {
            let mut sorted_left = left;
            let mut sorted_right = right;
            sorted_left.sort_unstable();
            sorted_right.sort_unstable();
            if sorted_left == sorted_right { return Ok(false); }
        }
    }
    Ok(true)
}

/// Admission checks before an ordinary server aggregate supplies any authority.
/// Keep opaque config bytes intact: decoding here validates, never normalizes.
pub(super) fn validate_notetype(n: &c::Notetype) -> Result<(), Error> {
    use meguri_model::proto::{BlobMessage, notetype::*};
    let config = NotetypeConfig::decode(&n.config).map_err(|_| Error::InvalidData)?;
    if n.struct_gen == 0 || n.fields.is_empty() || n.templates.is_empty()
        || !matches!(config.kind, 0 | 1) || config.sort_field_idx as usize >= n.fields.len()
        || n.name.is_empty() || std::str::from_utf8(&n.name).is_err()
    { return Err(Error::InvalidData); }
    for templates in [false, true] {
        let mut names = std::collections::BTreeSet::new();
        let parts: Box<dyn Iterator<Item = (i64, &[u8], &[u8])> + '_> = if templates {
            Box::new(n.templates.iter().map(|p| (p.ord, p.name.as_slice(), p.config.as_slice())))
        } else {
            Box::new(n.fields.iter().map(|p| (p.ord, p.name.as_slice(), p.config.as_slice())))
        };
        for (position, (ord, name, bytes)) in parts.enumerate() {
            let name = std::str::from_utf8(name).map_err(|_| Error::InvalidData)?;
            if ord != i64::try_from(position).map_err(|_| Error::InvalidData)? || name.is_empty()
                || !names.insert(meguri_text::fold::folded(name))
            { return Err(Error::InvalidData); }
            if templates { TemplateConfig::decode(bytes).map_err(|_| Error::InvalidData)?; }
            else { FieldConfig::decode(bytes).map_err(|_| Error::InvalidData)?; }
        }
    }
    n.encoded_len(false).map_err(codec_error)?;
    Ok(())
}

/// An absent retained image grants no ordinal map. Validate the native
/// aggregate independently before giving the typed collection-choice outcome.
pub(super) fn missing_layout(view: &impl View, id: i64) -> Result<c::Notetype, Error> {
    let head = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
    let actual = notetype(view, &head, 1)?;
    validate_notetype(&actual)?;
    Err(Error::ValidatedUnprovedLayout { notetype: id })
}

/// Classify only a fully validated aggregate mismatch. Exact retained images
/// remain ordinary authority; neither equal endpoints nor optional IDs infer a map.
pub(super) fn unproved_layout(actual: &c::Notetype, authority: &c::Notetype) -> Result<(), Error> {
    validate_notetype(actual)?;
    validate_notetype(authority)?;
    if actual.id != authority.id || actual.struct_gen != authority.struct_gen {
        return Err(Error::InvalidData);
    }
    Err(Error::ValidatedUnprovedLayout { notetype: actual.id })
}

/// Stable-layout runtime projection. The strict `project` API still verifies
/// exact images for unpublished stages and diagnostics.
pub fn project_ordinary(
    view: &impl View,
    kind: c::Kind,
    row: &[Cell],
    authority: impl FnMut(i64) -> Result<c::Notetype, Error>,
) -> Result<c::Payload, Error> {
    project_with(view, kind, row, authority, true)
}

pub fn project(
    view: &impl View,
    kind: c::Kind,
    row: &[Cell],
    acknowledged: impl FnMut(i64) -> Result<c::Notetype, Error>,
) -> Result<c::Payload, Error> {
    project_with(view, kind, row, acknowledged, false)
}
fn project_with(
    view: &impl View,
    kind: c::Kind,
    row: &[Cell],
    mut acknowledged: impl FnMut(i64) -> Result<c::Notetype, Error>,
    ordinary: bool,
) -> Result<c::Payload, Error> {
    let generation = |id, authority: &c::Notetype| -> Result<u64, Error> {
        if !ordinary { return layout(view, id, authority); }
        let head = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
        match notetype(view, &head, authority.struct_gen) {
            Ok(actual) if actual == *authority => Ok(authority.struct_gen),
            // Preserve classification of a malformed dependency as its own
            // failure; the reference eligibility fence still holds dependents.
            Err(Error::RowFailure(_)) => Ok(authority.struct_gen),
            Err(error) => Err(error),
            Ok(actual) => unproved_layout(&actual, authority).map(|()| authority.struct_gen),
        }
    };
    let t = table(kind).ok_or(Error::InvalidData)?;
    if row.len() != t.columns().len() {
        return Err(Error::InvalidData);
    }
    readable(t, row)?;
    use c::Payload as P;
    let p = match kind {
        c::Kind::Note => {
            let mid = int(row, 2)?;
            P::Note(super::runtime::note_image(
                row,
                generation(mid, &acknowledged(mid)?)?,
            )?)
        }
        c::Kind::Card => {
            let nid = int(row, 1)?;
            let note = view.row(Table::Notes, &RowKey::Id(nid))?.ok_or(Error::InvalidData)?;
            let mid = int(&note, 2)?;
            let generation = generation(mid, &acknowledged(mid)?)?;
            drop(note);
            P::Card(c::Card {
                id: int(row, 0)?,
                nid,
                did: int(row, 2)?,
                ord: int(row, 3)?,
                modified: int(row, 4)?,
                card_type: int(row, 6)?,
                queue: int(row, 7)?,
                due: stored(row, 8)?,
                ivl: int(row, 9)?,
                factor: int(row, 10)?,
                reps: int(row, 11)?,
                lapses: int(row, 12)?,
                left: int(row, 13)?,
                odue: stored(row, 14)?,
                odid: int(row, 15)?,
                flags: int(row, 16)?,
                data: stored(row, 17)?,
                last_review_id: view.history(int(row, 0)?)?,
                nt_struct_gen: generation,
            })
        }
        c::Kind::Revlog => P::Revlog(c::Revlog {
            id: int(row, 0)?,
            cid: int(row, 1)?,
            ease: int(row, 3)?,
            ivl: stored(row, 4)?,
            last_ivl: stored(row, 5)?,
            factor: int(row, 6)?,
            time: stored(row, 7)?,
            review_type: int(row, 8)?,
        }),
        c::Kind::Deck => {
            let name = bytes(row, 1, false)?;
            let mut parent_id = 0;
            if let Some(at) = name.iter().rposition(|b| *b == 31) {
                let parent = std::str::from_utf8(&name[..at]).map_err(|_| Error::InvalidData)?;
                let mut cursor = None;
                let mut found = false;
                loop {
                    let page = view.page(Table::Decks, cursor.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    for deck in page {
                        cursor = Some(RowKey::Id(int(&deck, 0)?));
                        let candidate = bytes(&deck, 1, false)?;
                        if meguri_text::fold::compare_folded(
                            parent,
                            std::str::from_utf8(&candidate).map_err(|_| Error::InvalidData)?,
                        ) == std::cmp::Ordering::Equal
                        {
                            if found {
                                return Err(Error::InvalidData);
                            }
                            parent_id = int(&deck, 0)?;
                            found = true;
                        }
                    }
                }
                if !found {
                    return Err(Error::InvalidData);
                }
            }
            P::Deck(c::Deck {
                id: int(row, 0)?,
                name,
                mtime_secs: int(row, 2)?,
                common: bytes(row, 4, true)?,
                kind: bytes(row, 5, true)?,
                parent_id,
            })
        }
        c::Kind::Preset => P::Preset(c::Preset {
            id: int(row, 0)?,
            name: bytes(row, 1, false)?,
            mtime_secs: int(row, 2)?,
            config: bytes(row, 4, true)?,
        }),
        c::Kind::Notetype => {
            let id = int(row, 0)?;
            let ack = acknowledged(id)?;
            let actual = notetype(view, row, ack.struct_gen)?;
            if actual != ack {
                if ordinary { unproved_layout(&actual, &ack)?; }
                else { return Err(Error::InvalidData); }
            }
            P::Notetype(actual)
        }
        c::Kind::Tag => P::Tag(c::Tag {
            tag: bytes(row, 0, false)?,
            collapsed: int(row, 2)?,
            config: match row.get(3) {
                Some(Cell::Null) => None,
                _ => Some(bytes(row, 3, true)?),
            },
        }),
        c::Kind::Config => P::Config(c::Config {
            key: bytes(row, 0, false)?,
            mtime_secs: int(row, 2)?,
            val: bytes(row, 3, true)?,
        }),
        c::Kind::MediaName => return Err(Error::InvalidData),
    };
    // The codec enforces the payload cap; envelope placement owns the inline limit.
    p.encoded_len().map_err(codec_error)?;
    Ok(p)
}

/// Identity of the real image, separate from a canonical payload (which may
/// not exist). Call only on a pinned reader or the committed writer view.
pub(super) fn raw_identity(view: &impl View, kind: c::Kind, row: &[Cell]) -> Result<[u8; 32], Error> {
    identity(view, kind, row, false)
}
pub(super) fn logical_identity(view: &impl View, kind: c::Kind, row: &[Cell]) -> Result<[u8; 32], Error> {
    identity(view, kind, row, true)
}
pub(super) fn logical_row_digest(table: Table, row: &[Cell]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for (i, cell) in row.iter().enumerate() {
        if Some(i) != table.usn_column() {
            hash.update((i as u64).to_le_bytes());
            hash.update(meguri_storage::journal::digest(std::slice::from_ref(cell)));
        }
    }
    hash.finalize().into()
}
fn identity(view: &impl View, kind: c::Kind, row: &[Cell], logical: bool) -> Result<[u8; 32], Error> {
    use sha2::{Digest, Sha256};
    let digest = |table, cells: &[Cell]| if logical { logical_row_digest(table, cells) } else { meguri_storage::journal::digest(cells) };
    let head = table(kind).ok_or(Error::InvalidData)?;
    if kind != c::Kind::Notetype { return Ok(digest(head, row)); }
    let mut hash = Sha256::new();
    hash.update(digest(head, row));
    if kind == c::Kind::Notetype {
        let id = int(row, 0)?;
        for t in [Table::Fields, Table::Templates] {
            hash.update(t.name().as_bytes());
            let mut after = None;
            loop {
                let page = view.raw_parts(t, id, after.as_ref())?;
                if page.is_empty() { break; }
                for part in page {
                    if int(&part, 0)? != id { return Err(Error::InvalidData); }
                    let ord = part.get(1).ok_or(Error::InvalidData)?.clone();
                    hash.update(digest(t, &part));
                    after = Some(ord);
                }
            }
        }
    }
    Ok(hash.finalize().into())
}

pub fn payload_key(payload: &c::Payload) -> Result<Vec<u8>, Error> {
    use c::Payload as P;
    Ok(match payload {
        P::Note(p) => c::id_key(p.id).to_vec(),
        P::Card(p) => c::id_key(p.id).to_vec(),
        P::Revlog(p) => c::id_key(p.id).to_vec(),
        P::Deck(p) => c::id_key(p.id).to_vec(),
        P::Preset(p) => c::id_key(p.id).to_vec(),
        P::Notetype(p) => c::id_key(p.id).to_vec(),
        P::Tag(p) => {
            meguri_text::fold::folded(std::str::from_utf8(&p.tag).map_err(|_| Error::InvalidData)?).into_bytes()
        }
        P::Config(p) => {
            std::str::from_utf8(&p.key).map_err(|_| Error::InvalidData)?;
            p.key.clone()
        }
        P::MediaName(p) => std::str::from_utf8(&p.name)
            .map_err(|_| Error::InvalidData)?
            .to_lowercase()
            .into_bytes(),
    })
}

/// Reverse the wire projection only into a caller-owned unpublished schema-18
/// stage. The storage scope owns SQL and preserves each SQLite storage class.
pub(super) fn stage_payload(scope: &mut meguri_storage::WriteScope<'_>, payload: &c::Payload) -> Result<(), Error> {
    for (table, key, row) in payload_rows(payload)? {
        scope.restore(table, &key, Some(&row))?;
    }
    Ok(())
}

/// Pure reverse projection shared by unpublished staging and journalled incoming ops.
pub(super) fn payload_rows(payload: &c::Payload) -> Result<Vec<(Table, RowKey, RowImage)>, Error> {
    use Cell::{Blob as B, Integer as I, Text as T};
    use c::{Payload as P, Stored as S};
    let stored = |s: &S| match s {
        S::Integer(v) => I(*v),
        S::Real(v) => Cell::Real(*v),
        S::Text(v) => T(v.clone()),
        S::Blob(v) => B(v.clone()),
    };
    let mut images = Vec::new();
    let mut put = |table, row: Vec<Cell>| -> Result<(), Error> {
        let key = RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?;
        images.push((table, key, row));
        Ok(())
    };
    match payload {
        P::Note(p) => put(
            Table::Notes,
            vec![
                I(p.id),
                T(p.guid.clone()),
                I(p.mid),
                I(p.modified),
                I(-1),
                T(p.tags.clone()),
                T(p.flds.clone()),
                stored(&p.sfld),
                I(p.csum),
                I(p.flags),
                T(p.data.clone()),
            ],
        )?,
        P::Card(p) => put(
            Table::Cards,
            vec![
                I(p.id),
                I(p.nid),
                I(p.did),
                I(p.ord),
                I(p.modified),
                I(-1),
                I(p.card_type),
                I(p.queue),
                stored(&p.due),
                I(p.ivl),
                I(p.factor),
                I(p.reps),
                I(p.lapses),
                I(p.left),
                stored(&p.odue),
                I(p.odid),
                I(p.flags),
                stored(&p.data),
            ],
        )?,
        P::Revlog(p) => put(
            Table::Revlog,
            vec![
                I(p.id),
                I(p.cid),
                I(-1),
                I(p.ease),
                stored(&p.ivl),
                stored(&p.last_ivl),
                I(p.factor),
                stored(&p.time),
                I(p.review_type),
            ],
        )?,
        P::Deck(p) => put(
            Table::Decks,
            vec![
                I(p.id),
                T(p.name.clone()),
                I(p.mtime_secs),
                I(-1),
                B(p.common.clone()),
                B(p.kind.clone()),
            ],
        )?,
        P::Preset(p) => put(
            Table::DeckConfig,
            vec![I(p.id), T(p.name.clone()), I(p.mtime_secs), I(-1), B(p.config.clone())],
        )?,
        P::Notetype(p) => {
            put(
                Table::Notetypes,
                vec![I(p.id), T(p.name.clone()), I(p.mtime_secs), I(-1), B(p.config.clone())],
            )?;
            for field in &p.fields {
                put(
                    Table::Fields,
                    vec![I(p.id), I(field.ord), T(field.name.clone()), B(field.config.clone())],
                )?;
            }
            for template in &p.templates {
                put(
                    Table::Templates,
                    vec![
                        I(p.id),
                        I(template.ord),
                        T(template.name.clone()),
                        I(template.mtime_secs),
                        I(-1),
                        B(template.config.clone()),
                    ],
                )?;
            }
        }
        P::Tag(p) => put(
            Table::Tags,
            vec![
                T(p.tag.clone()),
                I(-1),
                I(p.collapsed),
                p.config.clone().map(B).unwrap_or(Cell::Null),
            ],
        )?,
        P::Config(p) => put(
            Table::Config,
            vec![T(p.key.clone()), I(-1), I(p.mtime_secs), B(p.val.clone())],
        )?,
        P::MediaName(_) => return Err(Error::InvalidData),
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keys_agree_with_stored_row_keys() {
        for kind in [
            c::Kind::Note,
            c::Kind::Card,
            c::Kind::Revlog,
            c::Kind::Deck,
            c::Kind::Preset,
            c::Kind::Notetype,
        ] {
            for id in [i64::MIN, -1, 0, 1, i64::MAX] {
                assert_eq!(
                    tracking_key(kind, &RowKey::Id(id)).unwrap(),
                    key(kind, &[Cell::Integer(id)]).unwrap()
                );
            }
        }
        for (text, folded) in [
            ("Straße", "strasse"),
            ("\u{fb03}", "iff"),
            ("E\u{301}", "e\u{301}"),
            ("", ""),
        ] {
            let metadata = RowKey::text(text);
            let stored = [Cell::Text(text.as_bytes().to_vec())];
            for kind in [c::Kind::Tag, c::Kind::Config] {
                let actual = tracking_key(kind, &metadata).unwrap();
                assert_eq!(actual, key(kind, &stored).unwrap());
                assert_eq!(
                    actual,
                    if kind == c::Kind::Tag {
                        folded.as_bytes()
                    } else {
                        text.as_bytes()
                    }
                );
            }
        }
    }

    #[test]
    fn metadata_keys_refuse_wrong_shapes_and_invalid_utf8() {
        for kind in [
            c::Kind::Note,
            c::Kind::Card,
            c::Kind::Revlog,
            c::Kind::Deck,
            c::Kind::Preset,
            c::Kind::Notetype,
        ] {
            for metadata in [RowKey::text("key"), RowKey::Pair(1, 2)] {
                assert!(matches!(tracking_key(kind, &metadata), Err(Error::InvalidData)));
            }
        }
        for kind in [c::Kind::Tag, c::Kind::Config] {
            for metadata in [RowKey::Id(1), RowKey::Pair(1, 2)] {
                assert!(matches!(tracking_key(kind, &metadata), Err(Error::InvalidData)));
            }
        }
        for metadata in [RowKey::Id(1), RowKey::text("key"), RowKey::Pair(1, 2)] {
            assert!(matches!(
                tracking_key(c::Kind::MediaName, &metadata),
                Err(Error::InvalidData)
            ));
        }
        for bytes in [vec![0xff], vec![0xc3], vec![b'A', 0x80]] {
            for kind in [c::Kind::Tag, c::Kind::Config] {
                assert!(matches!(
                    tracking_key(kind, &RowKey::Text(bytes.clone())),
                    Err(Error::InvalidData)
                ));
            }
        }
    }
}

#[cfg(test)]
mod classification_tests {
    use super::*;
    struct Rows;
    impl View for Rows {
        fn row(&self, t: Table, _: &RowKey) -> Result<Option<RowImage>, Error> {
            Ok(match t {
                Table::Notetypes => Some(vec![Cell::Integer(2), Cell::text("Type"), Cell::Integer(0), Cell::Integer(-1), Cell::Blob(vec![])]),
                Table::Notes => Some(note()),
                _ => None,
            })
        }
        fn page(&self, _: Table, _: Option<&RowKey>) -> Result<Vec<RowImage>, Error> { Err(Error::Database) }
        fn parts(&self, _: Table, _: i64, _: Option<i64>) -> Result<Vec<RowImage>, Error> { Ok(vec![]) }
        fn history(&self, _: i64) -> Result<i64, Error> { Ok(0) }
    }
    fn ack(_: i64) -> Result<c::Notetype, Error> { notetype(&Rows, &Rows.row(Table::Notetypes, &RowKey::Id(2))?.unwrap(), 1) }
    fn note() -> RowImage {
        vec![Cell::Integer(1), Cell::Text(vec![0xff]), Cell::Integer(2), Cell::Integer(0), Cell::Integer(-1), Cell::text(""), Cell::text("q\x1fa"), Cell::Integer(1), Cell::Integer(-1), Cell::Integer(0), Cell::text("")]
    }
    #[test]
    fn k10_raw_readability_preserves_bytes_tolerances_and_distinguishes_dependencies() {
        let mut raw = note();
        let c::Payload::Note(projected) = project(&Rows, c::Kind::Note, &raw, ack).unwrap() else { panic!() };
        assert_eq!(projected.guid, vec![0xff]);
        assert_eq!(projected.csum, -1);
        let first = c::Payload::Note(projected);
        raw[4] = Cell::Integer(77);
        assert_eq!(project(&Rows, c::Kind::Note, &raw, ack).unwrap(), first, "usn stays out of the payload");
        raw[6] = Cell::Null;
        assert_eq!(key(c::Kind::Note, &raw).unwrap(), c::id_key(1));
        assert!(matches!(project(&Rows, c::Kind::Note, &raw, ack), Err(Error::RowFailure(RowFailure::Unreadable { table, column })) if table=="notes" && column=="flds"));
        assert!(matches!(project(&Rows, c::Kind::Note, &note(), |_| Err(Error::Database)), Err(Error::Database)));
        assert!(matches!(project(&Rows, c::Kind::Note, &note(), |_| Err(Error::InvalidData)), Err(Error::InvalidData)));
        let mut card = vec![Cell::Integer(0); Table::Cards.columns().len()];
        card[0]=Cell::Integer(3); card[1]=Cell::Integer(1);
        card[6]=Cell::Integer(258); // R4 takes the low eight bits.
        card[8]=Cell::Text(b"tolerated due".to_vec());
        card[17]=Cell::Blob(vec![0xff]);
        let c::Payload::Card(projected) = project(&Rows, c::Kind::Card, &card, ack).unwrap() else { panic!() };
        assert_eq!(projected.card_type, 258);
        assert_eq!(projected.due, c::Stored::Text(b"tolerated due".to_vec()));
        assert_eq!(projected.data, c::Stored::Blob(vec![0xff]));
        card[9]=Cell::Integer(-1);
        assert!(matches!(project(&Rows, c::Kind::Card, &card, ack), Err(Error::RowFailure(RowFailure::Unreadable { column, .. })) if column=="ivl"));
        let tolerant_config = [Cell::text("read-default"), Cell::Integer(-1), Cell::Integer(0), Cell::Integer(1)];
        assert!(matches!(project(&Rows, c::Kind::Config, &tolerant_config, ack), Err(Error::InvalidData)), "a non-BLOB config value is not an R9 unreadable row");
    }
    #[test]
    fn k10_notetype_raw_part_failure_is_distinct_from_layout_and_provider_errors() {
        struct Parts;
        impl View for Parts {
            fn row(&self, t: Table, k: &RowKey) -> Result<Option<RowImage>, Error> { Rows.row(t,k) }
            fn page(&self, t: Table, k: Option<&RowKey>) -> Result<Vec<RowImage>, Error> { Rows.page(t,k) }
            fn history(&self, id: i64) -> Result<i64, Error> { Rows.history(id) }
            fn parts(&self, t: Table, _: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
                Ok(if t==Table::Fields && after.is_none() { vec![vec![Cell::Integer(2), Cell::Integer(0), Cell::text("field"), Cell::Blob(vec![0xff])]] } else { vec![] })
            }
        }
        let raw = Rows.row(Table::Notetypes, &RowKey::Id(2)).unwrap().unwrap();
        assert!(matches!(project(&Parts, c::Kind::Notetype, &raw, ack), Err(Error::RowFailure(RowFailure::Unreadable { table, column })) if table=="fields" && column=="config"));
        let mut changed=ack(2).unwrap(); changed.name=b"other layout".to_vec();
        assert!(matches!(project(&Rows,c::Kind::Notetype,&raw, |_| Ok(changed.clone())),Err(Error::InvalidData)));
    }
    #[test]
    fn notetype_projection_and_raw_identity_continue_after_short_part_pages() {
        struct Parts { width: i64 }
        impl View for Parts {
            fn row(&self, t: Table, k: &RowKey) -> Result<Option<RowImage>, Error> { Rows.row(t, k) }
            fn page(&self, _: Table, _: Option<&RowKey>) -> Result<Vec<RowImage>, Error> { Err(Error::InvalidData) }
            fn history(&self, _: i64) -> Result<i64, Error> { Ok(0) }
            fn parts(&self, t: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
                let start = after.map_or(0, |ord| ord + 1);
                Ok((start..(start + self.width).min(250)).map(|ord| {
                    let mut row = vec![Cell::Integer(id), Cell::Integer(ord), Cell::text(format!("Part {ord}"))];
                    if t == Table::Templates { row.extend([Cell::Integer(0), Cell::Integer(-1)]); }
                    row.push(Cell::Blob(vec![]));
                    row
                }).collect())
            }
        }
        let head = Rows.row(Table::Notetypes, &RowKey::Id(2)).unwrap().unwrap();
        let full = Parts { width: 250 };
        let short = Parts { width: 6 };
        let projected = notetype(&short, &head, 1).unwrap();
        assert_eq!(projected.fields.len(), 250);
        assert_eq!(projected.templates.len(), 250);
        assert_eq!(projected, notetype(&full, &head, 1).unwrap());
        assert_eq!(raw_identity(&short, c::Kind::Notetype, &head).unwrap(), raw_identity(&full, c::Kind::Notetype, &head).unwrap());
        assert_eq!(logical_identity(&short, c::Kind::Notetype, &head).unwrap(), logical_identity(&full, c::Kind::Notetype, &head).unwrap());
    }
    #[test]
    fn k10_raw_canonical_cap_boundary_and_asset_placement_are_distinct() {
        // key tag+length+three bytes, val tag+four-byte length at 100 MiB.
        let cap = c::MAX_PAYLOAD_BYTES as usize;
        let mut raw = vec![Cell::text("cap"), Cell::Integer(-1), Cell::Integer(0), Cell::Blob(vec![0; cap-10])];
        {
            let payload = project(&Rows, c::Kind::Config, &raw, ack).unwrap();
            assert_eq!(payload.encoded_len().unwrap(), cap, "exactly 100 MiB is valid");
        }
        let Cell::Blob(value)=&mut raw[3] else { unreachable!() }; value.push(0);
        assert!(matches!(project(&Rows,c::Kind::Config,&raw,ack),Err(Error::RowFailure(RowFailure::Oversize))));
        drop(raw);
        let raw = vec![Cell::text("asset"), Cell::Integer(-1), Cell::Integer(0), Cell::Blob(vec![0; c::INLINE_ENVELOPE_BYTES])];
        let payload=project(&Rows,c::Kind::Config,&raw,ack).unwrap();
        let encoded=payload.encode().unwrap();
        let entity=c::Entity::build(c::EntityFields {
            kind:c::Kind::Config,key:b"asset".to_vec(),origin:[1;16],erev:1,generation:None,
            tombstone:false,deleted_at_ms:0,writer:[2;16],base_sha256:None,
        },&encoded).unwrap();
        assert!(entity.payload.is_none(), "inline threshold chooses an asset, never an oversize flag");
    }
}
