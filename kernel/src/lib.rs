#![no_std]

pub mod print;
pub mod serial;

/// Park the core until an interrupt arrives. Returns never.
pub fn wfi_loop() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
