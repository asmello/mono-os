#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;
use kernel::allocator::HEAP_SIZE;

entry_point!(main, config = &kernel::BOOTLOADER_CONFIG);

fn main(boot_info: &'static mut BootInfo) -> ! {
    use kernel::{
        allocator,
        memory::{self},
    };
    use x86_64::VirtAddr;

    kernel::init(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("enabled in config"),
        &boot_info.memory_regions,
    );
    test_main();

    kernel::hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[test_case]
fn simple_allocation() {
    let heap_value_1 = Box::new(41);
    let heap_value_2 = Box::new(13);
    assert_eq!(*heap_value_1, 41);
    assert_eq!(*heap_value_2, 13);
}

#[test_case]
fn large_vec() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    assert_eq!(vec.iter().sum::<u64>(), (n - 1) * n / 2);
}

#[test_case]
fn many_boxes() {
    for i in 0..HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
}
