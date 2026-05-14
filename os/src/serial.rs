use spin::{Lazy, Mutex};
use uart_16550::{Config, Uart16550Tty, backend::PioBackend};

pub static SERIAL1: Lazy<Mutex<Uart16550Tty<PioBackend>>> =
    Lazy::new(|| Mutex::new(unsafe { Uart16550Tty::new_port(0x3F8, Config::default()).unwrap() }));

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;

    crate::serial::SERIAL1.lock().write_fmt(args).unwrap();
}
