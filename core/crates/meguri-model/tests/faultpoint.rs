//! Fault points (R2003, KTD18): with `test-hooks`, an armed point aborts the
//! process or returns an injected I/O error; an unarmed one does nothing.

use std::io;
use std::process::Command;

use meguri_model::faultpoint;
use meguri_model::faultpoint::{Fault, arm, disarm, disarm_all};

fn write_step(name: &'static str) -> io::Result<u32> {
    faultpoint!(name)?;
    Ok(7)
}

#[test]
fn an_unarmed_point_does_nothing() {
    assert_eq!(write_step("test.unarmed").unwrap(), 7);
}

#[test]
fn an_armed_point_returns_the_injected_error_until_disarmed() {
    arm("test.io", Fault::IoError(io::ErrorKind::StorageFull));
    let error = write_step("test.io").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::StorageFull);
    assert!(error.to_string().contains("test.io"), "{error}");
    // Every hit fails while armed; other points are not affected.
    assert!(write_step("test.io").is_err());
    assert_eq!(write_step("test.io.other").unwrap(), 7);
    disarm("test.io");
    assert_eq!(write_step("test.io").unwrap(), 7);

    arm("test.io.all", Fault::IoError(io::ErrorKind::Other));
    disarm_all();
    assert_eq!(write_step("test.io.all").unwrap(), 7);
}

/// Run in a child process by the test below.
#[test]
fn abort_child() {
    if std::env::var_os("MEGURI_FAULTPOINT_ABORT_CHILD").is_none() {
        return;
    }
    arm("test.abort", Fault::Abort);
    let _ = write_step("test.abort");
    unreachable!("the armed point aborts");
}

#[test]
fn an_armed_abort_ends_the_process() {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["abort_child", "--exact", "--nocapture"])
        .env("MEGURI_FAULTPOINT_ABORT_CHILD", "1")
        .status()
        .unwrap();
    assert!(!status.success(), "{status:?}");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(6), "SIGABRT: {status:?}");
    }
}
