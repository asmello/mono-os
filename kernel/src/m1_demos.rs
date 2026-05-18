//! Hand-assembled user programs used to exercise the M1 sandbox.
//!
//! Each program lives as a small Thumb-2 byte block in flash with
//! start/end symbols. The kernel reads the bytes as a `&[u8]` and copies
//! them into the user_text region at launch. Only PC-relative branches
//! are used so the programs relocate cleanly to any user_text base.
//!
//! These programs are baked into the kernel binary so that
//! [`crate::main`] can exercise the runtime on real hardware (M1's
//! verification step). M2 will replace them with programs uploaded over
//! UART, at which point this module goes away.

use core::arch::global_asm;

use crate::user::UserProgram;

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
