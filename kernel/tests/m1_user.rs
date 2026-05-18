//! M1 sandbox tests: run five hand-assembled user programs through
//! [`kernel::user::launch`] and assert the abort status the kernel reports.
//!
//! Program bodies live in [`kernel::m1_demos`]; this file just wires each
//! demo to a `#[test_case]` that drives the kernel API and checks the
//! result.
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
use kernel::m1_demos;
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
    let r = user::launch(&m1_demos::exit_42());
    assert_eq!(r.status, AbortStatus::Ok, "payload={:#x}", r.payload);
    assert_eq!(r.payload, 42);
}

#[test_case]
fn user_reads_cycles() {
    let r = user::launch(&m1_demos::reads_cycles());
    assert_eq!(r.status, AbortStatus::Ok, "payload={:#x}", r.payload);
    // Payload is the delta from two SVC #1 calls. QEMU lm3s6965evb leaves
    // DWT stuck so the delta will be 0 there; on LPC1768 it's non-zero.
    // Shape-only check: just verify the syscall path returned cleanly.
}

#[test_case]
fn user_touches_kernel() {
    let r = user::launch(&m1_demos::touches_kernel());
    assert_eq!(r.status, AbortStatus::MemFault, "payload={:#x}", r.payload);
}

#[test_case]
fn user_executes_data() {
    let r = user::launch(&m1_demos::executes_data());
    assert_eq!(r.status, AbortStatus::MemFault, "payload={:#x}", r.payload);
}

#[test_case]
fn user_infinite_loop() {
    let r = user::launch(&m1_demos::infinite_loop());
    assert_eq!(r.status, AbortStatus::Timeout, "payload={:#x}", r.payload);
}
