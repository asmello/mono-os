#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(umami_os::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::BootInfo;
use umami_os::{allocator, memory};
use x86_64::VirtAddr;

bootloader::entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    umami_os::init();

    let mut mapper = unsafe { memory::init(VirtAddr::new(boot_info.physical_memory_offset)) };
    let mut frame_allocator = unsafe { memory::make_allocator(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).unwrap();

    #[cfg(test)]
    test_main();

    umami_os::hlt_loop();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    umami_os::println!("{info}");
    umami_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    umami_os::testing::test_panic_handler(info)
}

#[cfg(test)]
mod tests {
    #[test_case]
    #[allow(clippy::eq_op)]
    fn trivial_assertion() {
        assert_eq!(1, 1);
    }
}
