//! K6.1 projections of raw stored rows. These functions never write a collection.
use super::{Error, entity_codec as c};
use meguri_model::ids::{CardId, NotetypeId};
use meguri_storage::{Cell, ReadRows, RowImage, RowKey, Table};

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
pub fn notetype(view: &impl View, row: &[Cell], generation: u64) -> Result<c::Notetype, Error> {
    if generation == 0 {
        return Err(Error::InvalidData);
    }
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
    let mut total = n.name.len().checked_add(n.config.len()).ok_or(Error::InvalidData)?;
    for t in [Table::Fields, Table::Templates] {
        let mut after = None;
        loop {
            let page = view.parts(t, id, after)?;
            if page.is_empty() {
                break;
            }
            for p in page {
                let ord = int(&p, 1)?;
                if after.is_some_and(|a| ord <= a) || int(&p, 0)? != id {
                    return Err(Error::InvalidData);
                }
                let name = bytes(&p, 2, false)?;
                let config = bytes(&p, if t == Table::Fields { 3 } else { 5 }, true)?;
                total = total
                    .checked_add(name.len())
                    .and_then(|n| n.checked_add(config.len() + 64))
                    .ok_or(Error::InvalidData)?;
                if total > c::INLINE_ENVELOPE_BYTES {
                    return Err(Error::InvalidData);
                }
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
            }
        }
    }
    Ok(n)
}
/// An acknowledged generation is usable only when the complete raw layout agrees.
pub fn layout(view: &impl View, id: i64, acked: &c::Notetype) -> Result<u64, Error> {
    let row = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
    if notetype(view, &row, acked.struct_gen)? != *acked {
        return Err(Error::InvalidData);
    }
    Ok(acked.struct_gen)
}
pub fn project(
    view: &impl View,
    kind: c::Kind,
    row: &[Cell],
    mut acknowledged: impl FnMut(i64) -> Result<c::Notetype, Error>,
) -> Result<c::Payload, Error> {
    let t = table(kind).ok_or(Error::InvalidData)?;
    if row.len() != t.columns().len() {
        return Err(Error::InvalidData);
    }
    use c::Payload as P;
    let p = match kind {
        c::Kind::Note => {
            let mid = int(row, 2)?;
            P::Note(super::runtime::note_image(
                row,
                layout(view, mid, &acknowledged(mid)?)?,
            )?)
        }
        c::Kind::Card => {
            let nid = int(row, 1)?;
            let note = view.row(Table::Notes, &RowKey::Id(nid))?.ok_or(Error::InvalidData)?;
            let mid = int(&note, 2)?;
            let generation = layout(view, mid, &acknowledged(mid)?)?;
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
            layout(view, id, &ack)?;
            P::Notetype(ack)
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
    if p.encode().map_err(|_| Error::InvalidData)?.len() > c::INLINE_ENVELOPE_BYTES {
        return Err(Error::InvalidData);
    }
    Ok(p)
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
        P::MediaName(_) => return Err(Error::InvalidData),
    })
}
