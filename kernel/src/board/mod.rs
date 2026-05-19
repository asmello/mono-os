//! Board abstraction.
//!
//! Each supported board provides a zero-sized marker type that implements
//! [`Board`]; the feature flags pick exactly one of those as the public
//! [`Mcu`] alias. Callers reach the active board through `Mcu::…` rather
//! than naming a specific chip directly:
//!
//! ```ignore
//! use kernel::board::{Mcu, Board, TimeoutTimer};
//! Mcu::init();                              // calls `<Mcu as Board>::init`
//! Mcu::Timeout::arm_in_cycles(96_000);      // calls `<Mcu as Board>::Timeout`
//! ```
//!
//! Byte-level UART output goes through the [`UartTx`] trait, with the
//! [`LineMode`] adapter wrapping any `UartTx` in `fmt::Write` and
//! translating `\n` to `\r\n` so terminal output is well-formed on real
//! serial links.

use core::fmt;

pub trait Board {
    /// CPU core clock as observed at the state the kernel runs in (i.e. with
    /// whatever clock configuration `init` leaves behind, or the reset clock
    /// if `init` doesn't touch the clock tree).
    const CCLK_HZ: u32;

    /// Byte-level handle to the board's send side of the UART. A zero-sized
    /// type wrapping memory-mapped registers.
    type Tx: UartTx + Default;

    /// Interrupt-driven receive side of the same UART. The ISR pushes
    /// bytes into a ring buffer owned by the board module; consumers pop
    /// them with [`UartRx::read_byte`].
    type Rx: UartRx;

    /// One-shot deadline source. The kernel arms it before launching a user
    /// program and listens for its interrupt to enforce a timeout. The
    /// interrupt handler itself lives in the board module because its name
    /// depends on the chip; the handler acknowledges the peripheral and
    /// calls [`crate::timeout::mark_fired`].
    type Timeout: TimeoutTimer;

    /// One-time board bring-up: power, pin mux, baud, anything else the UART
    /// (or other always-on peripherals) needs before `print` can run.
    fn init();
}

pub trait UartTx {
    fn write_byte(&mut self, byte: u8);
}

/// Interrupt-driven UART receive. Each board's impl powers a small ring
/// buffer that the RX interrupt handler fills; [`read_byte`] is a
/// non-blocking pop for the consumer (the monolink frame parser).
pub trait UartRx {
    /// Enable the UART's receive interrupt and arm the NVIC line. The
    /// baud rate and pin mux are already set by [`Board::init`].
    fn init();

    /// Non-blocking pop. Returns `None` if the ring is empty.
    fn read_byte() -> Option<u8>;
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

/// `fmt::Write` adapter around any [`UartTx`]. Inserts a `\r` before every
/// `\n` so output renders correctly on raw serial terminals.
pub struct LineMode<C: UartTx>(pub C);

impl<C: UartTx> fmt::Write for LineMode<C> {
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
pub type Mcu = lm3s6965evb::Lm3s6965evb;

#[cfg(feature = "lpc1768")]
mod lpc1768;
#[cfg(feature = "lpc1768")]
pub type Mcu = lpc1768::Lpc1768;
