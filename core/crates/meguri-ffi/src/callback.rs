//! Rules for calls that cross into Swift (KTD9). Swift callback
//! implementations only yield into an `AsyncStream` continuation; they never
//! call the core synchronously. Every foreign callback runs inside [`invoke`],
//! and every export starts with [`enter`], so in debug builds a synchronous
//! call back into the core panics, and UniFFI hands Swift an internal error
//! that names the export and the callback.

use std::cell::Cell;

thread_local! {
    static IN_CALLBACK: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Called first by every export. In debug builds it panics when the export
/// runs synchronously inside a Swift callback.
#[inline]
pub(crate) fn enter(export: &'static str) {
    if cfg!(debug_assertions)
        && let Some(callback) = IN_CALLBACK.get()
    {
        panic!(
            "core export `{export}` was called synchronously from the Swift callback `{callback}`; \
             callbacks must only yield into an AsyncStream (KTD9)"
        );
    }
}

/// Runs a call into Swift. The marker is cleared again even if `call` unwinds.
pub(crate) fn invoke<R>(callback: &'static str, call: impl FnOnce() -> R) -> R {
    struct Reset(Option<&'static str>);
    impl Drop for Reset {
        fn drop(&mut self) {
            IN_CALLBACK.set(self.0);
        }
    }
    let _reset = Reset(IN_CALLBACK.replace(Some(callback)));
    call()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_outside_a_callback_passes() {
        enter("core_version");
    }

    #[test]
    #[cfg(debug_assertions)]
    fn enter_inside_a_callback_panics_with_both_names() {
        let caught = std::panic::catch_unwind(|| invoke("on_event", || enter("core_version")))
            .expect_err("a synchronous call back into the core must panic in debug builds");
        let message = caught.downcast_ref::<String>().expect("formatted panic message");
        assert!(message.contains("core_version"), "{message}");
        assert!(message.contains("on_event"), "{message}");
    }

    #[test]
    fn the_marker_is_cleared_after_the_callback_and_after_a_panic() {
        invoke("on_event", || {});
        enter("core_version");
        let _ = std::panic::catch_unwind(|| invoke("on_event", || panic!("callback failed")));
        enter("core_version");
    }
}
