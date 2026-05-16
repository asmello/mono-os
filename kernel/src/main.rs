#![no_std]
#![no_main]

use cortex_m_rt::entry;
use kernel::println;
use panic_semihosting as _;

#[entry]
fn main() -> ! {
    println!("hello, world");
    kernel::wfi_loop();
}
