//! Blocking TX-only driver for the Stellaris UART0 on the lm3s6965evb.
//!
//! Register layout per the LM3S6965 datasheet. QEMU's `lm3s6965evb` machine
//! emulates the UART as always-ready: writes to `UARTDR` are forwarded to the
//! `-serial` chardev with no clock or baud-rate setup required. The TXFF poll
//! is still useful: on real hardware we would not want to lose bytes, and the
//! cost in QEMU is one MMIO read per byte.

use core::fmt;
use core::ptr;

const UART0_BASE: usize = 0x4000_C000;
const UARTDR: *mut u32 = UART0_BASE as *mut u32;
const UARTFR: *const u32 = (UART0_BASE + 0x018) as *const u32;
const UARTFR_TXFF: u32 = 1 << 5;

pub struct Uart;

impl Uart {
    pub const fn new() -> Self {
        Uart
    }

    fn write_byte(&self, byte: u8) {
        unsafe {
            while ptr::read_volatile(UARTFR) & UARTFR_TXFF != 0 {}
            ptr::write_volatile(UARTDR, byte as u32);
        }
    }
}

impl Default for Uart {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let mut uart = Uart::new();
    let _ = uart.write_fmt(args);
}
