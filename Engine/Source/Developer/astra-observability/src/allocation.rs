use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocationSnapshot {
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
    pub allocated_bytes: u64,
    pub allocation_count: u64,
}

static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);

pub struct TrackingAllocator;

impl TrackingAllocator {
    pub const fn new() -> Self {
        Self
    }

    pub fn snapshot(&self) -> AllocationSnapshot {
        allocation_snapshot()
    }

    fn record_alloc(&self, bytes: usize) {
        let bytes = bytes as u64;
        let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
        ALLOCATED_BYTES.fetch_add(bytes, Ordering::Relaxed);
        ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
    }

    fn record_dealloc(&self, bytes: usize) {
        LIVE_BYTES.fetch_sub(bytes as u64, Ordering::Relaxed);
    }
}

pub fn allocation_snapshot() -> AllocationSnapshot {
    AllocationSnapshot {
        live_bytes: LIVE_BYTES.load(Ordering::Relaxed),
        peak_live_bytes: PEAK_LIVE_BYTES.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        allocation_count: ALLOCATION_COUNT.load(Ordering::Relaxed),
    }
}

impl Default for TrackingAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: every operation delegates to the process System allocator with the
// original pointer and layout. Atomics only observe byte counts and do not
// affect allocation ownership or lifetimes.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: delegated with the caller-provided valid layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            self.record_alloc(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        self.record_dealloc(layout.size());
        // SAFETY: delegated with the original allocation pointer and layout.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: delegated with the original pointer/layout and requested size.
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() {
            self.record_dealloc(layout.size());
            self.record_alloc(new_size);
        }
        replacement
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: delegated with the caller-provided valid layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            self.record_alloc(layout.size());
        }
        pointer
    }
}
