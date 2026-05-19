//! Deadline source for user-program execution.
//!
//! Wraps the active board's [`crate::board::TimeoutTimer`] with a
//! kernel-visible "fired" flag that the per-board interrupt handler sets
//! after acking the peripheral. M0 polls the flag from tests; M1+ will
//! hook the kernel-resume path off the same handler to abort the running
//! user program.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::board::{Board, Mcu, TimeoutTimer};

type Timer = <Mcu as Board>::Timeout;

static FIRED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    Timer::init();
}

/// Arm a one-shot deadline `cycles` CCLK cycles from now.
///
/// Resets [`fired`] so callers can wait on a fresh edge.
pub fn arm_in_cycles(cycles: u32) {
    FIRED.store(false, Ordering::Relaxed);
    Timer::arm_in_cycles(cycles);
}

pub fn disarm() {
    Timer::disarm();
}

pub fn fired() -> bool {
    FIRED.load(Ordering::Relaxed)
}

/// Called from the board's timer interrupt handler after it has acked the peripheral.
///
/// Safe to call multiple times.
pub fn mark_fired() {
    FIRED.store(true, Ordering::Relaxed);
}
