//! TimeoutTimer integration tests: arm + match interrupt + disarm.

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
    kernel::timeout::init();
    test_main();
    kernel::wfi_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn arm_fires() {
    // ~10 ms at 96 MHz; under QEMU it's whatever the emulated rate is, but
    // still well-bounded.
    kernel::timeout::arm_in_cycles(1_000_000);
    // Watchdog so a missed IRQ fails the test instead of hanging the runner.
    let mut spins = 0u32;
    while !kernel::timeout::fired() {
        spins = spins.wrapping_add(1);
        assert!(spins < 50_000_000, "timer never fired");
        cortex_m::asm::nop();
    }
}

#[test_case]
fn disarm_prevents_fire() {
    // Arm far out, then cancel immediately. The deadline should never trip.
    kernel::timeout::arm_in_cycles(10_000_000);
    cortex_m::asm::delay(1_000);
    kernel::timeout::disarm();
    // Wait well past where the arm would have fired.
    cortex_m::asm::delay(20_000_000);
    assert!(!kernel::timeout::fired(), "fired despite disarm");
}
