//! DWT cycle-counter integration tests.
//!
//! Note: QEMU's `lm3s6965evb` machine does not increment `DWT.CYCCNT` —
//! the register exists and is writable, but doesn't tick. So the rate
//! assertion is hardware-only; on QEMU we report `delta=0` and pass with
//! a note, while on real silicon (LPC1768) we assert sane bounds.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    kernel::init();
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::clock::init(p.DCB, p.DWT);
    test_main();
    kernel::wfi_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn cycles_api_callable() {
    let _ = kernel::clock::cycles_now();
    let _ = kernel::clock::cycles_now_u64();
}

#[test_case]
fn cycles_advance_or_skip() {
    let t0 = kernel::clock::cycles_now();
    cortex_m::asm::delay(10_000);
    let t1 = kernel::clock::cycles_now();
    let elapsed = t1.wrapping_sub(t0);
    kernel::println!(" [t0={t0} t1={t1} delta={elapsed}]");
    if elapsed == 0 {
        // DWT not implemented in this environment (QEMU lm3s6965evb).
        // Real hardware exercises the rate branch below.
        return;
    }
    assert!(elapsed >= 10_000, "elapsed {elapsed} < 10000");
    assert!(elapsed < 10_000_000, "elapsed {elapsed} suspiciously large");
}

#[test_case]
fn cycles_u64_advance_or_skip() {
    let t0 = kernel::clock::cycles_now_u64();
    cortex_m::asm::delay(10_000);
    let t1 = kernel::clock::cycles_now_u64();
    if t1 == t0 {
        // Same DWT-isn't-ticking caveat as above.
        return;
    }
    assert!(t1 > t0, "u64 cycles did not advance: {t0} → {t1}");
}
