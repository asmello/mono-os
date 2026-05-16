#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

#[cfg(test)]
use bootloader_api::BootInfo;
use bootloader_api::{BootloaderConfig, config::Mapping, info::MemoryRegions};

#[cfg(test)]
bootloader_api::entry_point!(test_kernel_main, config = &BOOTLOADER_CONFIG);

pub mod allocator;
pub mod gdt;
pub mod interrupts;
pub mod memory;
pub mod print;
pub mod qemu;
pub mod testing;

#[cfg(feature = "serial")]
pub mod serial;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use crate::testing::test_panic_handler;

    test_panic_handler(info)
}

#[cfg(test)]
fn test_kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use x86_64::VirtAddr;

    init(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("enabled in config"),
        &boot_info.memory_regions,
    );

    test_main();

    hlt_loop();
}

pub fn init(physical_memory_offset: u64, memory_regions: &'static MemoryRegions) {
    gdt::init();
    interrupts::init_idt();
    interrupts::pic::init();
    x86_64::instructions::interrupts::enable();
    let mut mapper = unsafe { memory::init(x86_64::VirtAddr::new(physical_memory_offset)) };
    let mut frame_allocator = unsafe { memory::make_allocator(memory_regions) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).unwrap();
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
