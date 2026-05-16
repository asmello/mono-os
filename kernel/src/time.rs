//! SysTick-driven monotonic tick counter.
//!
//! Reload value 11_999 → 12_000 cycles per exception. On the LM3S6965's reset
//! clock (PIOSC, nominally 12 MHz) that is 1 kHz; under QEMU the rate matches
//! whatever clock the emulator runs the core at, which is in the same order.
//! Accuracy is not a goal — `ticks()` is just a monotonically nondecreasing
//! counter that main code can poll between `wfi`s.

use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m::peripheral::SYST;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::exception;

static TICKS: AtomicU32 = AtomicU32::new(0);

const RELOAD_FOR_1_KHZ_AT_12_MHZ: u32 = 12_000 - 1;

pub fn init(mut syst: SYST) {
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(RELOAD_FOR_1_KHZ_AT_12_MHZ);
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
}
