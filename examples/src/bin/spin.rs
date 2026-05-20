//! Infinite loop — exists to exercise the hardware-timer-match deadline.
//!
//! The kernel should report `status=TIMEOUT` once `header.timeout_ms`
//! cycles elapse.

#![no_std]
#![no_main]

fn main() {
    // `volatile_read` keeps the loop body live so LLVM can't fold this into
    // a `wfi` or trap. (`wfi` would still timeout, but a loop with side
    // effects is a more faithful "user is computing" stand-in.)
    let counter = 0u32;
    loop {
        unsafe {
            core::ptr::read_volatile(&counter);
        }
    }
}

userlib::entry!(main);
