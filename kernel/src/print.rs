//! `println!` / `print!` macros and the underlying `_print` shim.
//!
//! `_print` constructs a fresh [`UartTx`](crate::board::UartTx) handle for
//! the active board on each call, wraps it in [`LineMode`] (so `\n` becomes
//! `\r\n`), and writes the formatted args through it.

use core::fmt;

use crate::board::{Board, LineMode, Mcu};

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let mut writer = LineMode(<Mcu as Board>::Tx::default());
    let _ = writer.write_fmt(args);
}
