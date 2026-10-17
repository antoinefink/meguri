//! Fault points (R2003, KTD18), a cross-area contract: every core unit puts
//! `faultpoint!("<name>")?` where a crash or an I/O failure must leave the
//! collection consistent. The macro is an `std::io::Result<()>` expression.
//!
//! Without the `test-hooks` feature a point is always `Ok(())` and compiles to
//! nothing. With it, a point the test runner armed aborts the process or
//! returns an injected I/O error. The point catalog and the kill runner belong
//! to U250.

/// Marks a fault point: `faultpoint!("storage.commit")?`.
#[macro_export]
macro_rules! faultpoint {
    ($name:expr) => {
        $crate::faultpoint::hit($name)
    };
}

/// What the macro calls. Without `test-hooks` it does nothing.
#[cfg(not(feature = "test-hooks"))]
#[inline(always)]
pub fn hit(_name: &'static str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(feature = "test-hooks")]
pub use armed::{Fault, arm, disarm, disarm_all, hit};

#[cfg(feature = "test-hooks")]
mod armed {
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// What an armed point does when it is reached.
    #[derive(Clone, Copy, Debug)]
    pub enum Fault {
        /// Ends the process at once, as a crash would.
        Abort,
        /// Test driver handshake at the exact boundary (for crash-consistent
        /// db/WAL captures). Never compiled into production.
        Callback(fn()),
        /// Returns an I/O error of this kind from the point.
        IoError(io::ErrorKind),
    }

    static ARMED: Mutex<Vec<(String, Fault)>> = Mutex::new(Vec::new());
    /// Whether anything is armed, so unarmed points never take the lock.
    static ANY_ARMED: AtomicBool = AtomicBool::new(false);

    fn armed() -> MutexGuard<'static, Vec<(String, Fault)>> {
        ARMED.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Arms the point `name` until it is disarmed: every time it is reached
    /// it does `fault`. Arming it again replaces its fault.
    pub fn arm(name: &str, fault: Fault) {
        let mut points = armed();
        points.retain(|(point, _)| point != name);
        points.push((name.to_owned(), fault));
        ANY_ARMED.store(true, Ordering::SeqCst);
    }

    pub fn disarm(name: &str) {
        let mut points = armed();
        points.retain(|(point, _)| point != name);
        ANY_ARMED.store(!points.is_empty(), Ordering::SeqCst);
    }

    pub fn disarm_all() {
        armed().clear();
        ANY_ARMED.store(false, Ordering::SeqCst);
    }

    /// What the macro calls.
    pub fn hit(name: &'static str) -> io::Result<()> {
        if !ANY_ARMED.load(Ordering::SeqCst) {
            return Ok(());
        }
        let fault = armed().iter().find(|(point, _)| point == name).map(|(_, fault)| *fault);
        match fault {
            None => Ok(()),
            Some(Fault::Abort) => std::process::abort(),
            Some(Fault::Callback(callback)) => {
                callback();
                Ok(())
            }
            Some(Fault::IoError(kind)) => Err(io::Error::new(kind, format!("injected fault at {name}"))),
        }
    }
}
