//! Optional harness witnesses; none counts as a production backup provider.
use meguri_engine::{Collection, EngineError, Seq, ports::BackupService};
use meguri_storage::{ReaderRole, raw::SqlValue};
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct BackupWitness {
    calls: AtomicUsize,
    record: AtomicBool,
    collection: Mutex<Option<Collection>>,
    checks: Mutex<Vec<(Seq, i64)>>,
}
impl BackupWitness {
    pub fn bind(&self, collection: Collection) {
        *self.collection.lock().unwrap_or_else(PoisonError::into_inner) = Some(collection);
    }
    pub fn record_checks(&self) {
        self.record.store(true, Ordering::SeqCst);
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
    /// The last committed sequence and live card count when each hook ran.
    pub fn checks(&self) -> Vec<(Seq, i64)> {
        self.checks.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}
impl BackupService for BackupWitness {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        if self.record.load(Ordering::SeqCst) {
            let collection = self
                .collection
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or_else(|| EngineError::Internal {
                    message: "backup witness is not bound".into(),
                })?;
            let reader = collection.reader(ReaderRole::Query)?;
            let snapshot = reader.snapshot()?;
            let rows = snapshot.execute_raw("SELECT count(*) FROM cards", &[])?;
            let [row] = rows.rows.as_slice() else {
                return Err(EngineError::Internal {
                    message: "invalid backup witness count".into(),
                });
            };
            let [SqlValue::Integer(count)] = row.as_slice() else {
                return Err(EngineError::Internal {
                    message: "invalid backup witness count".into(),
                });
            };
            self.checks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((snapshot.seq(), *count));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
