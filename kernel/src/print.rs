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

    #[cfg(feature = "serial")]
    crate::serial::_print(args);
}

#[cfg(feature = "serial")]
mod serial {
    #[macro_export]
    macro_rules! serial_print {
        ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
    }

    #[macro_export]
    macro_rules! serial_println {
        () => ($crate::serial_print!("\n"));
        ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
    }
}

pub mod testing {

    #[macro_export]
    macro_rules! tprint {
        ($($arg:tt)*) => ($crate::print::testing::_print(format_args!($($arg)*)));
    }

    #[macro_export]
    macro_rules! tprintln {
        () => ($crate::tprint!("\n"));
        ($($arg:tt)*) => ($crate::tprint!("{}\n", format_args!($($arg)*)));
    }

    #[doc(hidden)]
    pub fn _print(args: core::fmt::Arguments) {
        use core::fmt::Write;
        x86_64::instructions::interrupts::without_interrupts(|| {
            crate::testing::OUTPUT_CAPTURE
                .lock()
                .write_fmt(args)
                .unwrap();
        });
    }
}
