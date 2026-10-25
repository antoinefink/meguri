//! Observation/barrier hooks; all work still uses the production executor.
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug)]
pub enum Point {
    Snapshot(meguri_engine::Seq),
    Merged { table: meguri_storage::Table, rows: usize },
}
type Hook = Arc<dyn Fn(Point) + Send + Sync>;
static HOOK: Mutex<Option<Hook>> = Mutex::new(None);
pub struct Guard(Option<Hook>);
pub fn install(hook: Hook) -> Guard {
    Guard(HOOK.lock().unwrap().replace(hook))
}
pub(super) fn reach(point: Point) {
    let hook = HOOK.lock().unwrap().clone();
    if let Some(h) = hook {
        h(point);
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        *HOOK.lock().unwrap() = self.0.take();
    }
}
