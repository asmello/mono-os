//! M1 sandbox tests.
//!
//! Run five hand-assembled user programs through [`kernel::user::launch`]
//! and assert the abort status the kernel reports.
//!
//! The program bodies live in the [`demos`] module below — they're only
//! needed by this test, not the kernel proper.
//!
//! Caveat for QEMU: `lm3s6965evb` doesn't tick `DWT.CYCCNT`, so the
//! `user_reads_cycles` test only asserts shape, not magnitude. Real LPC1768
//! runs see a non-zero delta.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;
use kernel::user::{self, AbortStatus};

#[entry]
fn main() -> ! {
    kernel::init();
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::clock::init(p.DCB, p.DWT);
    kernel::timeout::init();
    test_main();
    kernel::wfi_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn user_exits_42() {
    let r = user::launch(&demos::exit_42());
    assert_eq!(r.status, AbortStatus::Ok, "payload={:#x}", r.payload);
    assert_eq!(r.payload, 42);
}

#[test_case]
fn user_reads_cycles() {
    let r = user::launch(&demos::reads_cycles());
    assert_eq!(r.status, AbortStatus::Ok, "payload={:#x}", r.payload);
    // Payload is the delta from two SVC #1 calls. QEMU lm3s6965evb leaves
    // DWT stuck so the delta will be 0 there; on LPC1768 it's non-zero.
    // Shape-only check: just verify the syscall path returned cleanly.
}

#[test_case]
fn user_touches_kernel() {
    let r = user::launch(&demos::touches_kernel());
    assert_eq!(r.status, AbortStatus::MemFault, "payload={:#x}", r.payload);
}

#[test_case]
fn user_executes_data() {
    let r = user::launch(&demos::executes_data());
    assert_eq!(r.status, AbortStatus::MemFault, "payload={:#x}", r.payload);
}

#[test_case]
fn user_infinite_loop() {
    let r = user::launch(&demos::infinite_loop());
    assert_eq!(r.status, AbortStatus::Timeout, "payload={:#x}", r.payload);
}

/// Hand-assembled user programs used to exercise the M1 sandbox.
///
/// Each program lives as a small Thumb-2 byte block in flash with
/// start/end symbols, baked into the test binary's `.text.m1_demos`
/// section. The kernel reads the bytes as a `&[u8]` and copies them into
/// the user_text region at launch. Only PC-relative branches are used so
/// the programs relocate cleanly to any user_text base.
mod demos {
    use core::arch::global_asm;
    use kernel::user::UserProgram;

    /// ~1 s at 96 MHz. Plenty of slack for the bounded programs.
    pub const DEFAULT_TIMEOUT_CYCLES: u32 = 100_000_000;
    /// ~10 ms at 96 MHz. Used by [`infinite_loop`] to trip the deadline fast.
    pub const TIGHT_TIMEOUT_CYCLES: u32 = 1_000_000;

    global_asm!(
        r#"
        .section .text.m1_demos, "ax"
        .syntax unified
        .thumb

        .align 1
        .global __m1_exit_42_start
        .global __m1_exit_42_end
    __m1_exit_42_start:
            movs    r0, #42
            svc     #0
    __m1_exit_42_end:

        .align 1
        .global __m1_reads_cycles_start
        .global __m1_reads_cycles_end
    __m1_reads_cycles_start:
            svc     #1
            mov     r4, r0
            movs    r5, #16
    1:
            subs    r5, r5, #1
            bne     1b
            svc     #1
            subs    r0, r0, r4
            svc     #0
    __m1_reads_cycles_end:

        .align 1
        .global __m1_touches_kernel_start
        .global __m1_touches_kernel_end
    __m1_touches_kernel_start:
            movs    r0, #0
            movs    r1, #42
            str     r1, [r0]
            movs    r0, #99
            svc     #0
    __m1_touches_kernel_end:

        .align 1
        .global __m1_executes_data_start
        .global __m1_executes_data_end
    __m1_executes_data_start:
            adr     r0, 1f
            add.w   r0, r0, #0x1000
            orr     r0, r0, #1
            bx      r0
            movs    r0, #99
            svc     #0
            .align 1
    1:
            .short  0xBE00
    __m1_executes_data_end:

        .align 1
        .global __m1_infinite_loop_start
        .global __m1_infinite_loop_end
    __m1_infinite_loop_start:
    1:
            b       1b
    __m1_infinite_loop_end:
        "#
    );

    unsafe extern "C" {
        static __m1_exit_42_start: u8;
        static __m1_exit_42_end: u8;
        static __m1_reads_cycles_start: u8;
        static __m1_reads_cycles_end: u8;
        static __m1_touches_kernel_start: u8;
        static __m1_touches_kernel_end: u8;
        static __m1_executes_data_start: u8;
        static __m1_executes_data_end: u8;
        static __m1_infinite_loop_start: u8;
        static __m1_infinite_loop_end: u8;
    }

    fn slice(start: &'static u8, end: &'static u8) -> &'static [u8] {
        let s = start as *const u8;
        let e = end as *const u8;
        unsafe { core::slice::from_raw_parts(s, e.offset_from(s) as usize) }
    }

    pub fn exit_42() -> UserProgram {
        UserProgram {
            text: slice(unsafe { &__m1_exit_42_start }, unsafe { &__m1_exit_42_end }),
            entry_offset: 0,
            timeout_cycles: DEFAULT_TIMEOUT_CYCLES,
        }
    }

    pub fn reads_cycles() -> UserProgram {
        UserProgram {
            text: slice(unsafe { &__m1_reads_cycles_start }, unsafe {
                &__m1_reads_cycles_end
            }),
            entry_offset: 0,
            timeout_cycles: DEFAULT_TIMEOUT_CYCLES,
        }
    }

    pub fn touches_kernel() -> UserProgram {
        UserProgram {
            text: slice(unsafe { &__m1_touches_kernel_start }, unsafe {
                &__m1_touches_kernel_end
            }),
            entry_offset: 0,
            timeout_cycles: DEFAULT_TIMEOUT_CYCLES,
        }
    }

    pub fn executes_data() -> UserProgram {
        UserProgram {
            text: slice(unsafe { &__m1_executes_data_start }, unsafe {
                &__m1_executes_data_end
            }),
            entry_offset: 0,
            timeout_cycles: DEFAULT_TIMEOUT_CYCLES,
        }
    }

    pub fn infinite_loop() -> UserProgram {
        UserProgram {
            text: slice(unsafe { &__m1_infinite_loop_start }, unsafe {
                &__m1_infinite_loop_end
            }),
            entry_offset: 0,
            timeout_cycles: TIGHT_TIMEOUT_CYCLES,
        }
    }
}
