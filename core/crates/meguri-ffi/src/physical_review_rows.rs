//! Test-only row observation. No tracking, runtime projection or writes.
use crate::CoreError;
use meguri_engine::Collection;
use meguri_model::ids::{CardId, RevlogId};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table};

/// Storage classes and bytes, including CardData's original text. Float bits
/// prevent observation from rounding a stored REAL. Only two bounded rows.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PhysicalReviewCell {
    Null,
    Integer { value: i64 },
    Real { bits: u64 },
    Text { bytes: Vec<u8> },
    Blob { bytes: Vec<u8> },
}
impl From<Cell> for PhysicalReviewCell {
    fn from(cell: Cell) -> Self {
        match cell {
            Cell::Null => Self::Null,
            Cell::Integer(value) => Self::Integer { value },
            Cell::Real(value) => Self::Real { bits: value.to_bits() },
            Cell::Text(bytes) => Self::Text { bytes },
            Cell::Blob(bytes) => Self::Blob { bytes },
        }
    }
}

/// A complete stored row in schema column order (model/row-records R4/R5).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PhysicalReviewRow {
    pub id: i64,
    pub cells: Vec<PhysicalReviewCell>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PhysicalReviewRows {
    pub generation: u64,
    pub seq: u64,
    pub card: Option<PhysicalReviewRow>,
    pub revlog: Option<PhysicalReviewRow>,
}

/// The Query lease excludes collection replacement. Its single O6 snapshot
/// pins both rows and the commit sequence; all returned cells are owned.
pub fn observe(collection: &Collection, card_id: i64, revlog_id: i64) -> Result<PhysicalReviewRows, CoreError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let card_id_read = snapshot.card(CardId(card_id))?.map(|row| row.id.0);
    let revlog_id_read = snapshot.revlog_entry(RevlogId(revlog_id))?.map(|row| row.id.0);
    let card = physical_row(
        snapshot.row_image(Table::Cards, &RowKey::Id(card_id))?,
        card_id_read,
        18,
    )?;
    let revlog = physical_row(
        snapshot.row_image(Table::Revlog, &RowKey::Id(revlog_id))?,
        revlog_id_read,
        9,
    )?;
    Ok(PhysicalReviewRows {
        generation: snapshot.generation().0,
        seq: snapshot.seq().0,
        card,
        revlog,
    })
}

fn physical_row(
    stored: Option<Vec<Cell>>,
    validated_id: Option<i64>,
    width: usize,
) -> Result<Option<PhysicalReviewRow>, CoreError> {
    match (stored, validated_id) {
        (None, None) => Ok(None),
        (Some(cells), Some(validated_id)) if cells.len() == width => {
            if let Some(Cell::Integer(id)) = cells.first()
                && *id == validated_id
            {
                return Ok(Some(PhysicalReviewRow {
                    id: *id,
                    cells: cells.into_iter().map(Into::into).collect(),
                }));
            }
            Err(row_shape_error())
        }
        _ => Err(row_shape_error()),
    }
}

fn row_shape_error() -> CoreError {
    CoreError::Internal {
        message: "physical review row shape mismatch".into(),
    }
}
