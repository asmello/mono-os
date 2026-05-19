//! High-resolution cycle counter via the ARMv7-M Data Watchpoint and Trace unit.
//!
//! `DWT.CYCCNT` increments once per CPU cycle, so at the active `CCLK_HZ` it
//! gives the finest timestamp available without a dedicated timer peripheral
//! — the measurement source the runtime reports back to the host.
//!
//! The raw counter is 32 bits and wraps after `2^32 / CCLK_HZ` seconds (~44 s
//! at 96 MHz). [`cycles_now_u64`] extends it in software: SysTick samples the
//! low word on every tick and bumps a high word whenever a wrap is observed.
//! As long as [`poll_extension`] is called more often than once per `2^32`
//! cycles, no wrap can be missed.

use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m::peripheral::{DCB, DWT};

static HIGH: AtomicU32 = AtomicU32::new(0);
static LAST_LOW: AtomicU32 = AtomicU32::new(0);

pub fn init(mut dcb: DCB, mut dwt: DWT) {
    dcb.enable_trace();
    dwt.set_cycle_count(0);
    dwt.enable_cycle_counter();
}

#[inline]
pub fn cycles_now() -> u32 {
    DWT::cycle_count()
}

/// Sample the low word and detect rollover since the previous sample.
///
/// Called from the SysTick handler. Idempotent if the counter hasn't moved.
pub fn poll_extension() {
    let low = DWT::cycle_count();
    let last = LAST_LOW.swap(low, Ordering::Relaxed);
    if low < last {
        HIGH.fetch_add(1, Ordering::Relaxed);
    }
}

/// Read the software-extended 64-bit cycle count.
///
/// Tolerates being preempted by [`poll_extension`] between the high-word
/// reads: if the high word changed mid-read, re-snapshot the low word
/// against the post-wrap high.
pub fn cycles_now_u64() -> u64 {
    let h0 = HIGH.load(Ordering::Relaxed);
    let l = DWT::cycle_count();
    let h1 = HIGH.load(Ordering::Relaxed);
    if h0 == h1 {
        ((h0 as u64) << 32) | l as u64
    } else {
        ((h1 as u64) << 32) | DWT::cycle_count() as u64
    }
}
