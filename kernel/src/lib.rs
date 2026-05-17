#![no_std]

pub mod board;
pub mod clock;
pub mod print;
pub mod testing;
pub mod time;

/// One-time board setup. Brings up the UART so `println!` works.
pub fn init() {
    board::init();
}

/// Park the core until an interrupt arrives. Returns never.
pub fn wfi_loop() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
