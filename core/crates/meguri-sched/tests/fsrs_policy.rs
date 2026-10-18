use meguri_sched::compat::fsrs_policy::{PolicyStatus, status};

#[test]
fn policy_status_distinguishes_first_enable_from_another_client_turning_it_off() {
    assert_eq!(status(false, false), PolicyStatus::NeedsEnable);
    assert_eq!(status(false, true), PolicyStatus::NeedsReEnable);
    assert_eq!(status(true, false), PolicyStatus::Ok);
    assert_eq!(status(true, true), PolicyStatus::Ok);
}
