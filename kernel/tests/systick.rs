#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::time::init(p.SYST);
    test_main();
    kernel::wfi_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn ticks_advance() {
    let start = kernel::time::ticks();
    while kernel::time::ticks() < start + 10 {
        cortex_m::asm::wfi();
    }
    assert!(kernel::time::ticks() >= start + 10);
}
