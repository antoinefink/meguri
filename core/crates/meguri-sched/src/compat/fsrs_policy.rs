//! The three states of sched/fsrs-policy S1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyStatus {
    Ok,
    NeedsEnable,
    NeedsReEnable,
}

pub fn status(enabled: bool, seen_on: bool) -> PolicyStatus {
    match (enabled, seen_on) {
        (true, _) => PolicyStatus::Ok,
        (false, false) => PolicyStatus::NeedsEnable,
        (false, true) => PolicyStatus::NeedsReEnable,
    }
}
