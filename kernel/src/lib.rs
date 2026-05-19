#![no_std]
#![cfg_attr(test, no_main)]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(crate::testing::test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

pub mod board;
pub mod clock;
pub mod link;
pub mod mpu;
pub mod print;
pub mod ring;
pub mod testing;
pub mod time;
pub mod timeout;
pub mod user;

/// One-time board setup.
///
/// Brings up the UART (TX + interrupt-driven RX), so `println!` works and
/// bytes coming in over the wire are buffered into the ring for
/// [`Mcu::Rx::read_byte`]. Also arms the individually vectored MemManage /
/// BusFault / UsageFault handlers that [`user`] relies on.
pub fn init() {
    use board::{Board, UartRx};
    <board::Mcu as Board>::init();
    <<board::Mcu as Board>::Rx as UartRx>::init();
    user::init();
}

/// Park the core until an interrupt arrives. Returns never.
pub fn wfi_loop() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

#[cfg(test)]
#[cortex_m_rt::entry]
fn main() -> ! {
    init();
    test_main();
    wfi_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    testing::test_panic_handler(info)
}
