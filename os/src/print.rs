use core::fmt;

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
#[allow(unused)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    #[cfg(feature = "vga-text")]
    crate::vga_buffer::WRITER.lock().write_fmt(args).unwrap();

    #[cfg(feature = "serial")]
    crate::serial::SERIAL1.lock().write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::print::_serial_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

#[cfg(feature = "serial")]
#[doc(hidden)]
pub fn _serial_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    crate::serial::SERIAL1.lock().write_fmt(args).unwrap();
}

#[cfg(not(feature = "serial"))]
#[doc(hidden)]
pub fn _serial_print(_args: core::fmt::Arguments) {}
