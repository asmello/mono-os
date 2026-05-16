//! Blocking TX-only driver for the Stellaris UART0 on the lm3s6965evb.
//!
//! Register layout per the LM3S6965 datasheet. QEMU's `lm3s6965evb` machine
//! emulates the UART as always-ready: writes to `UARTDR` are forwarded to the
//! `-serial` chardev with no clock or baud-rate setup required. The TXFF poll
//! is still useful on real hardware, and costs one MMIO read per byte under
//! QEMU.

use core::ptr;

const UART0_BASE: usize = 0x4000_C000;
const UARTDR: *mut u32 = UART0_BASE as *mut u32;
const UARTFR: *const u32 = (UART0_BASE + 0x018) as *const u32;
const UARTFR_TXFF: u32 = 1 << 5;

pub struct Lm3s6965evb;

impl super::Board for Lm3s6965evb {
    /// Approximate emulated core clock under QEMU's `lm3s6965evb` machine.
    /// The chip's real reset clock is PIOSC at 6 MHz, but QEMU's stellaris
    /// timing is not authoritative — this is a ballpark value chosen to give
    /// a roughly visible tick rate.
    const CCLK_HZ: u32 = 12_000_000;

    type Uart = Uart;

    fn init() {
        // QEMU's emulated Stellaris UART needs no setup.
    }
}

#[derive(Default)]
pub struct Uart;

impl super::Console for Uart {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while ptr::read_volatile(UARTFR) & UARTFR_TXFF != 0 {}
            ptr::write_volatile(UARTDR, byte as u32);
        }
    }
}
