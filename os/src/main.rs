#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(umami_os::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    umami_os::println!("{info}");
    loop {}
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    umami_os::testing::test_panic_handler(info)
}

#[unsafe(no_mangle)]
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    umami_os::init();

    #[cfg(test)]
    test_main();

    loop {}
}

#[cfg(test)]
mod tests {
    #[test_case]
    #[allow(clippy::eq_op)]
    fn trivial_assertion() {
        assert_eq!(1, 1);
    }
}
