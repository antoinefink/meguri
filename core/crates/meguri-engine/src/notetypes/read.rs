//! Reading note types (`specs/notetypes/records.md` N1): every note type,
//! their names, a note type by its stored name (`specs/conformance/
//! notetype-ops.md` T1) and the collection's Image Occlusion note type (N9),
//! on any connection: a reader, a snapshot or an op's write scope.

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, StorageError, Table};

use super::StockKind;

/// Note types are few: one page holds them all.
const PAGE: usize = 1_000;

/// Every note type's id, ascending.
pub fn ids<R: ReadRows + ?Sized>(reader: &R) -> Result<Vec<NotetypeId>, StorageError> {
    let mut ids = Vec::new();
    let mut after = None;
    loop {
        let page = reader.ids_after(Table::Notetypes, after, PAGE)?;
        let Some(last) = page.last().copied() else {
            return Ok(ids);
        };
        after = Some(last);
        ids.extend(page.into_iter().map(NotetypeId));
    }
}

/// Every note type with its fields and templates, by id.
pub fn all<R: ReadRows + ?Sized>(reader: &R) -> Result<Vec<Notetype>, StorageError> {
    let mut found = Vec::new();
    for id in ids(reader)? {
        if let Some(notetype) = reader.notetype(id)? {
            found.push(notetype);
        }
    }
    Ok(found)
}

/// Every note type's id and stored name, by id.
pub fn names<R: ReadRows + ?Sized>(reader: &R) -> Result<Vec<(NotetypeId, String)>, StorageError> {
    Ok(all(reader)?.into_iter().map(|n| (n.id, n.name)).collect())
}

/// The note type stored under exactly this name (no case folding).
pub fn by_name<R: ReadRows + ?Sized>(reader: &R, name: &str) -> Result<Option<Notetype>, StorageError> {
    Ok(all(reader)?.into_iter().find(|n| n.name == name))
}

/// N9: the note type with the lowest id among those of stock kind 6 with at
/// least 4 fields.
pub fn image_occlusion<R: ReadRows + ?Sized>(reader: &R) -> Result<Option<Notetype>, StorageError> {
    Ok(all(reader)?
        .into_iter()
        .find(|n| n.config.original_stock_kind == StockKind::ImageOcclusion.stored() && n.fields.len() >= 4))
}
