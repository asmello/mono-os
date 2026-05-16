//! `harness = false` regression test: panicking is the success path.
//!
//! `main` triggers a panic via a function call (not a direct `panic!()` whose
//! `!` return type the compiler would let us prove dead code with). If the
//! panic handler fires we report `[ok]` and exit `EXIT_SUCCESS`. If execution
//! ever falls through to the line after the trigger, we report failure.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use cortex_m_semihosting::debug;

#[entry]
fn main() -> ! {
    kernel::print!("should_panic::panics_reach_handler...");
    cause_panic();
    kernel::println!("[failed: did not panic]");
    debug::exit(debug::EXIT_FAILURE);
    kernel::wfi_loop();
}

fn cause_panic() {
    panic!("deliberate");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    kernel::println!("[ok]");
    debug::exit(debug::EXIT_SUCCESS);
    kernel::wfi_loop();
}
