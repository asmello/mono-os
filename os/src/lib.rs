#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod print;

#[cfg(feature = "serial")]
mod serial;
#[cfg(feature = "vga-text")]
mod vga_buffer;

pub mod testing;

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    test_panic_handler(info)
}

#[cfg(test)]
#[unsafe(no_mangle)]
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    #[cfg(test)]
    test_main();

    loop {}
}

#[allow(clippy::empty_loop)]
pub fn test_panic_handler(info: &core::panic::PanicInfo) -> ! {
    println!("[failed]\n");
    println!("Error: {info}\n");
    exit_qemu(QemuExitCode::Failed);
    loop {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}
