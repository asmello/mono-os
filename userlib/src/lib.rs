//! Runtime support for monolink user programs.
//!
//! Provides the privilege-side counterparts to the syscalls defined in
//! `kernel/src/user.rs` and an [`entry`] macro that pins a user-supplied
//! `_start` at offset 0 of the user_text region (so the kernel can launch
//! with `entry_offset = 0`).
//!
//! Usage skeleton:
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//!
//! fn main() {
//!     userlib::exit(42);
//! }
//!
//! userlib::entry!(main);
//! ```
//!
//! See `examples/` for runnable programs.

#![no_std]

use core::arch::asm;

/// Terminate the user program with `code` as the exit status (syscall #0).
#[inline(always)]
pub fn exit(code: u32) -> ! {
    unsafe {
        asm!(
            "svc #0",
            in("r0") code,
            options(noreturn),
        )
    }
}

/// Read the monotonic CPU cycle counter since program launch (syscall #1).
///
/// The counter is reset to 0 at launch by the kernel. On `lm3s6965evb`
/// (QEMU) DWT.CYCCNT is stuck, so the result is always 0 there; on real
/// LPC1768 hardware it increments at CCLK.
#[inline(always)]
pub fn time_cycles() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!(
            "svc #1",
            out("r0") lo,
            out("r1") hi,
            options(nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Pull bytes from the case input buffer (syscall #2).
///
/// Copies up to `buf.len()` bytes into `buf` and returns how many were
/// copied. Subsequent calls drain the rest of the input incrementally;
/// once everything has been consumed `read` returns 0.
#[inline(always)]
pub fn read(buf: &mut [u8]) -> usize {
    let ptr = buf.as_mut_ptr();
    let max = buf.len() as u32;
    let n: u32;
    unsafe {
        asm!(
            "svc #2",
            inout("r0") ptr => n,
            in("r1") max,
            options(nostack, preserves_flags),
        );
    }
    n as usize
}

/// Append bytes to the case output buffer (syscall #3).
///
/// Multiple calls concatenate. The kernel ships everything written before
/// `exit()` back to the host as part of the case's RESULT. Writing past
/// the device-side cap (currently 1 KiB) aborts the run with a usage
/// fault.
#[inline(always)]
pub fn write(buf: &[u8]) {
    let ptr = buf.as_ptr();
    let len = buf.len() as u32;
    unsafe {
        asm!(
            "svc #3",
            in("r0") ptr,
            in("r1") len,
            options(nostack, preserves_flags),
        );
    }
}

/// Pin a function as the user-program entry point.
///
/// Emits a `pub extern "C" fn _start()` in the `.text._start` section that
/// calls the supplied function, then [`exit(0)`](exit) if it returns. The
/// linker script in `user.x` anchors `.text._start` at offset 0 of the
/// user_text region so the kernel can launch with `entry_offset = 0`.
#[macro_export]
macro_rules! entry {
    ($main:ident) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".text._start")]
        pub extern "C" fn _start() -> ! {
            $main();
            $crate::exit(0)
        }
    };
}

/// Default panic handler: exit with `0xFFFF_FFFF`.
///
/// Feature-gated behind `panic-handler` (on by default). Disable the
/// feature to provide your own `#[panic_handler]`.
#[cfg(feature = "panic-handler")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(0xFFFF_FFFF)
}
