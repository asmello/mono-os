//! Measure a small busy loop via the `time_cycles` syscall.
//!
//! Calls `time_cycles` before and after a 1024-iteration volatile load
//! loop and exits with the low 32 bits of the delta. The kernel-reported
//! cycle count (in the RESULT frame) bounds this from above — the user-
//! reported delta is purely the inner loop, the kernel-reported number
//! includes the surrounding SVC overhead.
//!
//! On QEMU `lm3s6965evb` the DWT cycle counter is stuck, so both deltas
//! read 0; on real LPC1768 hardware both are non-zero.

#![no_std]
#![no_main]

fn main() {
    let start = userlib::time_cycles();
    let counter = 0u32;
    for _ in 0..1024 {
        unsafe {
            core::ptr::read_volatile(&counter);
        }
    }
    let delta = userlib::time_cycles().wrapping_sub(start);
    userlib::exit(delta as u32);
}

userlib::entry!(main);
