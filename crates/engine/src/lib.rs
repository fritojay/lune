//! This crate must stay free of Qt types and platform-specific (`std::os::*`) APIs.

mod convert;
mod decoder;
mod error;
mod mp4_gapless;
mod output;
mod player;
mod queue;
mod render;
mod source;
#[cfg(test)]
#[path = "tests/test_util.rs"]
mod test_util;
mod timeline;
mod units;

pub use error::{EngineError, Result};
pub use output::{OutputDevice, OutputKind, output_devices};
pub use player::{PlaybackState, Player, PlayerConfig, Status};
pub use queue::{EntryId, PlayQueue, QueueEntry, VecQueue};

/// Test-only allocator that counts allocations per thread, so tests can
/// prove realtime code paths never allocate.
#[cfg(test)]
pub(crate) mod alloc_counter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        // `const` init: reading it never allocates (no lazy initialization).
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }

    pub struct Counting;

    // SAFETY: forwards every call to `System` unchanged; only counts.
    #[expect(unsafe_code, reason = "a GlobalAlloc impl is unsafe by definition")]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            COUNT.with(|c| c.set(c.get() + 1));
            // SAFETY: same contract as the caller's.
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: same contract as the caller's.
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            COUNT.with(|c| c.set(c.get() + 1));
            // SAFETY: same contract as the caller's.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn count() -> u64 {
        COUNT.with(Cell::get)
    }
}
