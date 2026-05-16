#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader_api::BootInfo;
use kernel::{
    BOOTLOADER_CONFIG, println,
    qemu::{QemuExitCode, exit_qemu},
};

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    println!("Entered kernel with boot info: {boot_info:?}");

    kernel::init(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("enabled in config"),
        &boot_info.memory_regions,
    );

    #[cfg(test)]
    test_main();

    println!("\nEverything set up!\n");
    exit_qemu(QemuExitCode::Success);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::println!("{info}");
    kernel::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[cfg(test)]
mod tests {
    #[test_case]
    #[allow(clippy::eq_op)]
    fn trivial_assertion() {
        assert_eq!(1, 1);
    }
}
