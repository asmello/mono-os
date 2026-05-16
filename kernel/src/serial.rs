use spin::{LazyLock, Mutex};
use uart_16550::{Config, Uart16550Tty, backend::PioBackend};

pub static SERIAL1: LazyLock<Mutex<Uart16550Tty<PioBackend>>> = LazyLock::new(|| {
    Mutex::new(unsafe { Uart16550Tty::new_port(0x3F8, Config::default()).unwrap() })
});

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        crate::serial::SERIAL1.lock().write_fmt(args).unwrap();
    });
}
