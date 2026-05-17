//! SysTick-driven monotonic tick counter.
//!
//! The reload value is derived from each board's `CCLK_HZ` constant so the
//! tick rate is honest on hardware. `ticks()` is a monotonically nondecreasing
//! counter that main code can poll between `wfi`s.

use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m::peripheral::{SYST, syst::SystClkSource};
use cortex_m_rt::exception;

use crate::board::{Board, Mcu};

static TICKS: AtomicU32 = AtomicU32::new(0);

const TICK_HZ: u32 = 1_000;

pub fn init(mut syst: SYST) {
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(Mcu::CCLK_HZ / TICK_HZ - 1);
    syst.clear_current();
    syst.enable_counter();
    syst.enable_interrupt();
}

pub fn ticks() -> u32 {
    TICKS.load(Ordering::Relaxed)
}

#[exception]
fn SysTick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::clock::poll_extension();
}
