#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader_api::{BootInfo, entry_point};

entry_point!(main, config = &kernel::BOOTLOADER_CONFIG);

fn main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::init();
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
