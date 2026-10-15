//! A counting global allocator for benches: the system allocator plus the
//! bytes live now and the most live since the last [`PeakAlloc::reset_peak`].
//! It sees only Rust's allocations; SQLite's page cache and statements come
//! from its own allocator calls, so a bench reports this beside the process
//! footprint as a breakdown, never as the memory number (R1864).
//!
//! A bench binary installs it with
//! `#[global_allocator] static ALLOC: PeakAlloc = PeakAlloc;`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The counting allocator. Its counters are process-wide.
#[derive(Clone, Copy, Debug, Default)]
pub struct PeakAlloc;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

impl PeakAlloc {
    /// Bytes allocated through it and not yet freed.
    pub fn current() -> usize {
        CURRENT.load(Ordering::Relaxed)
    }

    /// The most bytes live at once since the last reset.
    pub fn peak() -> usize {
        PEAK.load(Ordering::Relaxed)
    }

    /// Starts a new peak from what is live now.
    pub fn reset_peak() {
        PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// Whether this process allocates through it (it is the global
    /// allocator): only then do its numbers mean anything.
    pub fn is_installed() -> bool {
        INSTALLED.load(Ordering::Relaxed)
    }

    fn added(size: usize) {
        INSTALLED.store(true, Ordering::Relaxed);
        let now = CURRENT.fetch_add(size, Ordering::Relaxed) + size;
        PEAK.fetch_max(now, Ordering::Relaxed);
    }

    fn removed(size: usize) {
        CURRENT.fetch_sub(size, Ordering::Relaxed);
    }
}

// SAFETY: every call goes to the system allocator with the same layout; the
// counters only observe sizes.
unsafe impl GlobalAlloc for PeakAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds alloc's contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            PeakAlloc::added(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            PeakAlloc::added(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; ptr came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
        PeakAlloc::removed(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged; ptr came from this allocator.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            PeakAlloc::removed(layout.size());
            PeakAlloc::added(new_size);
        }
        new
    }
}
