//! Board abstraction.
//!
//! Each supported board provides a zero-sized marker type that implements
//! [`Board`]. Feature flags pick exactly one of them as the [`Active`] board.
//! The public surface (`CCLK_HZ`, `init`, `_print`) is computed from `Active`,
//! so callers never name a specific board.
//!
//! Byte-level UART output goes through the [`Console`] trait. The
//! [`LineMode`] adapter wraps any `Console` in `fmt::Write` and translates
//! `\n` to `\r\n` so terminal output is well-formed on real serial links.

use core::fmt;

pub trait Board {
    /// CPU core clock as observed at the state the kernel runs in (i.e. with
    /// whatever clock configuration `init` leaves behind, or the reset clock
    /// if `init` doesn't touch the clock tree).
    const CCLK_HZ: u32;

    /// Byte-level handle to the board's console UART. A zero-sized type
    /// wrapping memory-mapped registers.
    type Uart: Console + Default;

    /// One-shot deadline source. The kernel arms it before launching a user
    /// program and listens for its interrupt to enforce a timeout. The
    /// interrupt handler itself lives in the board module because its name
    /// depends on the chip; the handler acknowledges the peripheral and
    /// calls [`crate::timeout::mark_fired`].
    type Timeout: TimeoutTimer;

    /// One-time board bring-up: power, pin mux, baud, anything else the UART
    /// (or other always-on peripherals) needs before `_print` can run.
    fn init();
}

pub trait Console {
    fn write_byte(&mut self, byte: u8);
}

/// Single entry in the Cortex-M device-interrupt vector table. The slot is
/// either a handler function pointer or a reserved-zero word.
///
/// cortex-m-rt 0.7 ships a private `Vector` union of the same shape but
/// keeps its fields `doc(hidden)`, so we mirror it here with public fields
/// to populate `.vector_table.interrupts` directly without going through a
/// PAC. The layout must match what the Cortex-M boot ROM expects.
#[repr(C)]
pub union IrqVector {
    pub handler: unsafe extern "C" fn(),
    pub reserved: usize,
}

/// Hardware timer programmable as a one-shot match interrupt expressed in
/// CCLK cycles. Used to enforce per-user-program deadlines without
/// perturbing the measurement (one IRQ at most, fired only on overflow).
pub trait TimeoutTimer {
    /// One-time setup: power on the peripheral, set its clock divider so a
    /// match register tick equals one CCLK cycle, enable its NVIC line, and
    /// leave it disarmed.
    fn init();

    /// Schedule a match interrupt after `cycles` CCLK cycles from this call.
    /// Replaces any previous arm.
    fn arm_in_cycles(cycles: u32);

    /// Cancel any pending match. Idempotent.
    fn disarm();
}

/// `fmt::Write` adapter around any [`Console`]. Inserts a `\r` before every
/// `\n` so output renders correctly on raw serial terminals.
pub struct LineMode<C: Console>(pub C);

impl<C: Console> fmt::Write for LineMode<C> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.0.write_byte(b'\r');
            }
            self.0.write_byte(byte);
        }
        Ok(())
    }
}

#[cfg(feature = "lm3s6965evb")]
mod lm3s6965evb;
#[cfg(feature = "lm3s6965evb")]
type Active = lm3s6965evb::Lm3s6965evb;

#[cfg(feature = "lpc1768")]
mod lpc1768;
#[cfg(feature = "lpc1768")]
type Active = lpc1768::Lpc1768;

pub const CCLK_HZ: u32 = Active::CCLK_HZ;

pub fn init() {
    Active::init();
}

pub fn timeout_init() {
    <Active as Board>::Timeout::init();
}

pub fn timeout_arm_in_cycles(cycles: u32) {
    <Active as Board>::Timeout::arm_in_cycles(cycles);
}

pub fn timeout_disarm() {
    <Active as Board>::Timeout::disarm();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let mut writer = LineMode(<Active as Board>::Uart::default());
    let _ = writer.write_fmt(args);
}
