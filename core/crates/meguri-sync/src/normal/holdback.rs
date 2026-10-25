//! H4's captured snapshot and lazy oldest-before-image overlay.
use super::{Result, engine_error, storage_error};
use meguri_engine::{CancelToken, SyncHold};
use meguri_model::clock::Clock;
use meguri_storage::{ReadRows, Table, normal_stage::NormalStage};

pub(super) fn build(
    stage: &NormalStage,
    snapshot: &impl ReadRows,
    hold: &SyncHold,
    clock: &dyn Clock,
    cancel: &CancelToken,
) -> Result<()> {
    for table in Table::ALL {
        let mut after = None;
        loop {
            cancel.check().map_err(engine_error)?;
            let page = snapshot.row_page(table, after.as_ref()).map_err(storage_error)?;
            if page.is_empty() {
                break;
            }
            after = page.last().map(|r| NormalStage::row_key(table, r));
            stage.copy_page(table, &page).map_err(storage_error)?;
        }
    }
    stage.write(clock, |scope| -> Result<()> {
        // Remove the whole held set before restoring, so a rename cannot
        // conflict with another name created later in the held run.
        for row in hold.run.rows() {
            cancel.check().map_err(engine_error)?;
            let row = row.map_err(engine_error)?;
            scope.restore(row.table, &row.key, None).map_err(storage_error)?;
        }
        for row in hold.run.rows() {
            cancel.check().map_err(engine_error)?;
            let row = row.map_err(engine_error)?;
            scope
                .restore(row.table, &row.key, row.before.as_ref())
                .map_err(storage_error)?;
        }
        Ok(())
    })
}
