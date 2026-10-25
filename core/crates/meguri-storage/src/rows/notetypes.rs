//! Note types (`model/row-records` R7): the `notetypes` row with its
//! `fields` and `templates` rows, ordered by `ord`. Template rows carry
//! `mtime_secs` and `usn` as data, never stamped (`ids-usn-mtime` S1).

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use rusqlite::Connection;

use super::{Cell, RowCells, RowImage, Table};
use crate::error::StorageError;

/// The `notetypes` row of a note type.
pub(crate) fn head_cells(notetype: &Notetype) -> RowImage {
    vec![
        Cell::Integer(notetype.id.0),
        Cell::text(notetype.name.clone()),
        Cell::Integer(notetype.mtime.0),
        Cell::Integer(i64::from(notetype.usn.0)),
        Cell::Blob(notetype.config.to_bytes().into_owned()),
    ]
}

pub(crate) fn field_cells(ntid: NotetypeId, field: &NotetypeField) -> RowImage {
    vec![
        Cell::Integer(ntid.0),
        Cell::Integer(i64::from(field.ord)),
        Cell::text(field.name.clone()),
        Cell::Blob(field.config.to_bytes().into_owned()),
    ]
}

pub(crate) fn template_cells(ntid: NotetypeId, template: &NotetypeTemplate) -> RowImage {
    vec![
        Cell::Integer(ntid.0),
        Cell::Integer(i64::from(template.ord)),
        Cell::text(template.name.clone()),
        Cell::Integer(template.mtime.0),
        Cell::Integer(i64::from(template.usn.0)),
        Cell::Blob(template.config.to_bytes().into_owned()),
    ]
}

/// Reads a note type with its fields and templates.
pub(crate) fn get(conn: &Connection, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
    get_prefixed(conn, "", id)
}

pub(crate) fn get_prefixed(conn: &Connection, prefix: &str, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
    let read = |table: Table| -> Result<Vec<RowImage>, StorageError> {
        let columns = table
            .columns()
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        let key = if table == Table::Notetypes { "id" } else { "ntid" };
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {columns} FROM {prefix}{} WHERE {key}=?1 ORDER BY {}",
            table.name(),
            if key == "id" { "id" } else { "ord" }
        ))?;
        Ok(stmt
            .query_map([id.0], |r| super::row_image(r, table))?
            .collect::<rusqlite::Result<_>>()?)
    };
    let Some(head) = read(Table::Notetypes)?.pop() else {
        return Ok(None);
    };
    let row = RowCells {
        table: Table::Notetypes,
        cells: &head,
    };
    let mut notetype = Notetype {
        id: NotetypeId(row.int(0)?),
        name: row.text(1)?,
        mtime: EpochSecs(row.int(2)?),
        usn: Usn(row.int(3)?),
        config: row.wrap(4, ProtoBlob::decode(row.blob(4)?))?,
        fields: Vec::new(),
        templates: Vec::new(),
    };
    for cells in read(Table::Fields)? {
        let row = RowCells {
            table: Table::Fields,
            cells: &cells,
        };
        notetype.fields.push(NotetypeField {
            ord: row.int(1)?,
            name: row.text(2)?,
            config: row.wrap(3, ProtoBlob::decode(row.blob(3)?))?,
            prior_ord: None,
        });
    }
    for cells in read(Table::Templates)? {
        let row = RowCells {
            table: Table::Templates,
            cells: &cells,
        };
        notetype.templates.push(NotetypeTemplate {
            ord: row.int(1)?,
            name: row.text(2)?,
            mtime: EpochSecs(row.int(3)?),
            usn: Usn(row.int(4)?),
            config: row.wrap(5, ProtoBlob::decode(row.blob(5)?))?,
            prior_ord: None,
        });
    }
    Ok(Some(notetype))
}
