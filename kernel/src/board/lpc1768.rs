//! Blocking TX-only driver for UART0 on the mbed NXP LPC1768.
//!
//! UART0 is wired to P0.2/P0.3, which the mbed interface MCU bridges to the
//! USB CDC ACM device exposed to the host (the `/dev/cu.usbmodem*` port).
//! 9600 8N1, the mbed factory default.
//!
//! Init sequence per LPC1768 UM10360 chapter 14:
//!   1. PCONP[3]   — UART0 power. (Set on at reset; we re-assert for clarity.)
//!   2. PCLKSEL0[7:6] = 01 — UART0 peripheral clock = CCLK/1 = 4 MHz at reset
//!      (CCLK comes up on IRC, 4 MHz).
//!   3. PINSEL0[5:4]=01, [7:6]=01 — wire P0.2→TXD0, P0.3→RXD0.
//!   4. LCR.DLAB=1; DLL=26, DLM=0 — 4 MHz / (16·26) ≈ 9615 baud (0.2% error).
//!   5. LCR = 0x03   — 8 bits, 1 stop, no parity, DLAB cleared.
//!   6. FCR = 0x07   — enable + reset both FIFOs.
//!   7. IER = 0      — interrupts off (we poll).
//!
//! TX path: poll LSR[5] (THRE) until set, then write THR.

use core::ptr;

const PCONP: *mut u32 = 0x400F_C0C4 as *mut u32;
const PCLKSEL0: *mut u32 = 0x400F_C1A8 as *mut u32;
const PINSEL0: *mut u32 = 0x4002_C000 as *mut u32;

const UART0_BASE: usize = 0x4000_C000;
const UART0_THR: *mut u32 = UART0_BASE as *mut u32;
const UART0_DLL: *mut u32 = UART0_BASE as *mut u32;
const UART0_IER: *mut u32 = (UART0_BASE + 0x04) as *mut u32;
const UART0_DLM: *mut u32 = (UART0_BASE + 0x04) as *mut u32;
const UART0_FCR: *mut u32 = (UART0_BASE + 0x08) as *mut u32;
const UART0_LCR: *mut u32 = (UART0_BASE + 0x0C) as *mut u32;
const UART0_LSR: *const u32 = (UART0_BASE + 0x14) as *const u32;

const PCONP_PCUART0: u32 = 1 << 3;
const PCLKSEL0_UART0_CCLK_DIV1: u32 = 0b01 << 6;
const PCLKSEL0_UART0_MASK: u32 = 0b11 << 6;
const PINSEL0_P0_2_TXD0: u32 = 0b01 << 4;
const PINSEL0_P0_3_RXD0: u32 = 0b01 << 6;
const PINSEL0_P0_2_P0_3_MASK: u32 = 0xF << 4;
const LCR_DLAB: u32 = 1 << 7;
const LCR_8N1: u32 = 0b11;
const FCR_FIFO_ENABLE_AND_RESET: u32 = 0x07;
const LSR_THRE: u32 = 1 << 5;
const BAUD_DIVISOR_9600_AT_4MHZ: u32 = 26;

pub struct Lpc1768;

impl super::Board for Lpc1768 {
    /// Core clock frequency at reset: the LPC176x runs from the IRC at 4 MHz
    /// (data sheet §8.29.1.1; ±1 % trimmed) until software configures the
    /// PLL. We leave the PLL off, so CCLK = IRC = 4 MHz for the kernel.
    const CCLK_HZ: u32 = 4_000_000;

    type Uart = Uart;

    fn init() {
        unsafe {
            ptr::write_volatile(PCONP, ptr::read_volatile(PCONP) | PCONP_PCUART0);

            let mut pclksel = ptr::read_volatile(PCLKSEL0);
            pclksel = (pclksel & !PCLKSEL0_UART0_MASK) | PCLKSEL0_UART0_CCLK_DIV1;
            ptr::write_volatile(PCLKSEL0, pclksel);

            let mut pinsel = ptr::read_volatile(PINSEL0);
            pinsel = (pinsel & !PINSEL0_P0_2_P0_3_MASK) | PINSEL0_P0_2_TXD0 | PINSEL0_P0_3_RXD0;
            ptr::write_volatile(PINSEL0, pinsel);

            ptr::write_volatile(UART0_LCR, LCR_DLAB);
            ptr::write_volatile(UART0_DLL, BAUD_DIVISOR_9600_AT_4MHZ);
            ptr::write_volatile(UART0_DLM, 0);
            ptr::write_volatile(UART0_LCR, LCR_8N1);
            ptr::write_volatile(UART0_FCR, FCR_FIFO_ENABLE_AND_RESET);
            ptr::write_volatile(UART0_IER, 0);
        }
    }
}

#[derive(Default)]
pub struct Uart;

impl super::Console for Uart {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while ptr::read_volatile(UART0_LSR) & LSR_THRE == 0 {}
            ptr::write_volatile(UART0_THR, byte as u32);
        }
    }
}
