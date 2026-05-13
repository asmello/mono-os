#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

mod interrupts;
pub mod print;
pub mod qemu;
pub mod testing;

#[cfg(feature = "serial")]
mod serial;
#[cfg(feature = "vga-text")]
mod vga_buffer;

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use crate::testing::test_panic_handler;

    test_panic_handler(info)
}

#[cfg(test)]
#[unsafe(no_mangle)]
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    init();
    test_main();
    loop {}
}

pub fn init() {
    interrupts::init_idt();
}
