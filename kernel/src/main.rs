#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;

#[cfg(not(test))]
#[entry]
fn main() -> ! {
    kernel::println!("hello, world");
    kernel::wfi_loop();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::println!();
    kernel::println!("KERNEL PANIC");
    kernel::println!("{info}");
    kernel::wfi_loop();
}

#[cfg(test)]
#[entry]
fn main() -> ! {
    test_main();
    kernel::wfi_loop();
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
