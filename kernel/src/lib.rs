#![no_std]

pub mod board;
pub mod clock;
pub mod m1_demos;
pub mod mpu;
pub mod print;
pub mod testing;
pub mod time;
pub mod timeout;
pub mod user;

/// One-time board setup. Brings up the UART so `println!` works and arms
/// the individually-vectored MemManage / BusFault / UsageFault handlers
/// that [`user`] relies on.
pub fn init() {
    <board::Mcu as board::Board>::init();
    user::init();
}

/// Park the core until an interrupt arrives. Returns never.
pub fn wfi_loop() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
