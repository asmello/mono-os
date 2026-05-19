//! SysTick-driven monotonic tick counter.
//!
//! The reload value is derived from each board's `CCLK_HZ` constant so the
//! tick rate is honest on hardware. `ticks()` is a monotonically nondecreasing
//! counter that main code can poll between `wfi`s.
//!
//! [`pause`] / [`resume`] gate the interrupt around user-program execution:
//! periodic ticks would perturb the cycle-count measurement, so the kernel
//! silences SysTick before launching unprivileged code and restores it on
//! the way back.

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use cortex_m::peripheral::{SYST, syst::SystClkSource};
use cortex_m_rt::exception;

use crate::board::{Board, Mcu};

static TICKS: AtomicU32 = AtomicU32::new(0);

const TICK_HZ: u32 = 1_000;

// SysTick Control and Status Register (ARMv7-M ARM B3.3.1).
const SYST_CSR: *mut u32 = 0xE000_E010 as *mut u32;
const SYST_CSR_ENABLE: u32 = 1 << 0;
const SYST_CSR_TICKINT: u32 = 1 << 1;

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

/// Disable the SysTick counter+interrupt for the duration of a user-program run.
///
/// Returns the prior enable state so a matching [`resume`] only fires when
/// the kernel had actually enabled SysTick to begin with.
pub fn pause() -> bool {
    unsafe {
        let csr = ptr::read_volatile(SYST_CSR);
        let was_enabled = csr & SYST_CSR_ENABLE != 0;
        if was_enabled {
            ptr::write_volatile(SYST_CSR, csr & !(SYST_CSR_ENABLE | SYST_CSR_TICKINT));
        }
        was_enabled
    }
}

pub fn resume() {
    unsafe {
        let csr = ptr::read_volatile(SYST_CSR);
        ptr::write_volatile(SYST_CSR, csr | SYST_CSR_ENABLE | SYST_CSR_TICKINT);
    }
}

#[exception]
fn SysTick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::clock::poll_extension();
}
