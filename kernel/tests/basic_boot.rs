#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[unsafe(no_mangle)] // don't mangle the name of this function
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    test_main();
    kernel::hlt_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn test_println() {
    kernel::println!("test_println output");
}
