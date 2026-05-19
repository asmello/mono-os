//! Board driver for the mbed NXP LPC1768 (Cortex-M3).
//!
//! `init()` brings CCLK from the 4 MHz IRC up to 96 MHz off the mbed's 12 MHz
//! crystal via PLL0, then configures UART0 for 115200 8N1 against the new
//! clock. Register names and sequence numbers refer to UM10360 (LPC176x/5x
//! user manual).
//!
//! PLL0 sizing (UM10360 §4.5):
//!   M = 12, N = 1 → Fcco = 2·M·Fin/N = 288 MHz (within the 275–550 MHz band).
//!   CCLK divider = 3 → CCLK = 96 MHz. Crystal-stable, ~50 ppm.
//!
//! UART0 baud at 96 MHz PCLK: 96e6 / (16 · 52) ≈ 115_384, ~0.16% above 115200
//! — well within standard async tolerance. The 52 divisor splits to DLM = 0,
//! DLL = 52.
//!
//! UART0 is wired to P0.2/P0.3, which the mbed interface MCU bridges to the
//! host's USB CDC ACM port (the `/dev/cu.usbmodem*` device).

use crate::ring::SpscRing;
use core::ptr;
use cortex_m::peripheral::NVIC;
use lpc176x::{Interrupt, interrupt};

// System control block (UM10360 §3.3, base 0x400F_C000).
const FLASHCFG: *mut u32 = 0x400F_C000 as *mut u32;
const PLL0CON: *mut u32 = 0x400F_C080 as *mut u32;
const PLL0CFG: *mut u32 = 0x400F_C084 as *mut u32;
const PLL0STAT: *const u32 = 0x400F_C088 as *const u32;
const PLL0FEED: *mut u32 = 0x400F_C08C as *mut u32;
const PCONP: *mut u32 = 0x400F_C0C4 as *mut u32;
const CCLKCFG: *mut u32 = 0x400F_C104 as *mut u32;
const CLKSRCSEL: *mut u32 = 0x400F_C10C as *mut u32;
const SCS: *mut u32 = 0x400F_C1A0 as *mut u32;
const PCLKSEL0: *mut u32 = 0x400F_C1A8 as *mut u32;
const PINSEL0: *mut u32 = 0x4002_C000 as *mut u32;

// SCS (UM10360 §3.3.2): main oscillator control.
const SCS_OSCRANGE_MASK: u32 = 1 << 4; // 0 = 1–20 MHz, our 12 MHz fits.
const SCS_OSCEN: u32 = 1 << 5;
const SCS_OSCSTAT: u32 = 1 << 6;

// CLKSRCSEL (§3.3.4): clock source for PLL0.
const CLKSRCSEL_MAIN_OSC: u32 = 0b01;

// PLL0 control & status (§4.5.10–13).
const PLL0CON_PLLE: u32 = 1 << 0;
const PLL0CON_PLLC: u32 = 1 << 1;
const PLL0STAT_PLOCK: u32 = 1 << 26;

// FLASHCFG (§3.3.7): FLASHTIM in bits 15:12. Value 4 = 5 CCLK wait states,
// safe up to 100 MHz.
const FLASHCFG_FLASHTIM_5CLK: u32 = 4 << 12;
const FLASHCFG_FLASHTIM_MASK: u32 = 0xF << 12;

// PLL0CFG (§4.5.11): M in bits 14:0, N in bits 23:16. Encoded as M-1 / N-1,
// so M=12,N=1 maps to MSEL=11,NSEL=0.
const PLL0CFG_M12_N1: u32 = 11;
// CCLKCFG (§4.7.1): CCLK divider = field + 1, so /3 maps to field=2.
const CCLKCFG_DIV3: u32 = 2;

// PCLKSEL0 (§4.7.3): peripheral clock selectors. Each field is 2 bits;
// 01 → CCLK/1. TIMER0 occupies bits 3:2, UART0 bits 7:6.
const PCLKSEL0_TIMER0_CCLK_DIV1: u32 = 0b01 << 2;
const PCLKSEL0_TIMER0_MASK: u32 = 0b11 << 2;
const PCLKSEL0_UART0_CCLK_DIV1: u32 = 0b01 << 6;
const PCLKSEL0_UART0_MASK: u32 = 0b11 << 6;

// Timer0 (UM10360 ch. 21, base 0x4000_4000).
const TIMER0_BASE: usize = 0x4000_4000;
const T0_IR: *mut u32 = TIMER0_BASE as *mut u32;
const T0_TCR: *mut u32 = (TIMER0_BASE + 0x04) as *mut u32;
const T0_PR: *mut u32 = (TIMER0_BASE + 0x0C) as *mut u32;
const T0_MCR: *mut u32 = (TIMER0_BASE + 0x14) as *mut u32;
const T0_MR0: *mut u32 = (TIMER0_BASE + 0x18) as *mut u32;

const PCONP_PCTIM0: u32 = 1 << 1;
const TCR_ENABLE: u32 = 1 << 0;
const TCR_RESET: u32 = 1 << 1;
const MCR_MR0I: u32 = 1 << 0; // interrupt on MR0 match; do not stop or reset.
const IR_MR0: u32 = 1 << 0;

// UART0 (UM10360 ch. 14, base 0x4000_C000).
const UART0_BASE: usize = 0x4000_C000;
const UART0_RBR: *const u32 = UART0_BASE as *const u32;
const UART0_THR: *mut u32 = UART0_BASE as *mut u32;
const UART0_DLL: *mut u32 = UART0_BASE as *mut u32;
const UART0_IER: *mut u32 = (UART0_BASE + 0x04) as *mut u32;
const UART0_DLM: *mut u32 = (UART0_BASE + 0x04) as *mut u32;
const UART0_IIR: *const u32 = (UART0_BASE + 0x08) as *const u32;
const UART0_FCR: *mut u32 = (UART0_BASE + 0x08) as *mut u32;
const UART0_LCR: *mut u32 = (UART0_BASE + 0x0C) as *mut u32;
const UART0_LSR: *const u32 = (UART0_BASE + 0x14) as *const u32;

const PCONP_PCUART0: u32 = 1 << 3;
const PINSEL0_P0_2_TXD0: u32 = 0b01 << 4;
const PINSEL0_P0_3_RXD0: u32 = 0b01 << 6;
const PINSEL0_P0_2_P0_3_MASK: u32 = 0xF << 4;
const LCR_DLAB: u32 = 1 << 7;
const LCR_8N1: u32 = 0b11;
const FCR_FIFO_ENABLE_AND_RESET: u32 = 0x07;
const LSR_THRE: u32 = 1 << 5;
const LSR_RDR: u32 = 1 << 0;
const IER_RBR: u32 = 1 << 0; // RX data available interrupt.
const IIR_INTID_MASK: u32 = 0b1110;
const IIR_INTID_RDA: u32 = 0b0100; // Receive Data Available.
const IIR_INTID_CTI: u32 = 0b1100; // Character Time-out Indicator.
const BAUD_DLM_115200_AT_96MHZ: u32 = 0;
const BAUD_DLL_115200_AT_96MHZ: u32 = 52;

unsafe fn feed_pll0() {
    unsafe {
        ptr::write_volatile(PLL0FEED, 0xAA);
        ptr::write_volatile(PLL0FEED, 0x55);
    }
}

fn setup_clock_96mhz() {
    unsafe {
        // Flash accelerator: raise wait states BEFORE raising CCLK, otherwise
        // instruction fetches at 96 MHz with too few wait states will fail.
        let flashcfg = ptr::read_volatile(FLASHCFG);
        ptr::write_volatile(
            FLASHCFG,
            (flashcfg & !FLASHCFG_FLASHTIM_MASK) | FLASHCFG_FLASHTIM_5CLK,
        );

        // Enable the main oscillator (12 MHz crystal on the mbed) and wait
        // for SCS.OSCSTAT to confirm it's stable.
        let scs = ptr::read_volatile(SCS);
        ptr::write_volatile(SCS, (scs & !SCS_OSCRANGE_MASK) | SCS_OSCEN);
        while ptr::read_volatile(SCS) & SCS_OSCSTAT == 0 {}

        ptr::write_volatile(CLKSRCSEL, CLKSRCSEL_MAIN_OSC);

        // Configure, enable, wait for PLL0 lock.
        ptr::write_volatile(PLL0CFG, PLL0CFG_M12_N1);
        feed_pll0();
        ptr::write_volatile(PLL0CON, PLL0CON_PLLE);
        feed_pll0();
        while ptr::read_volatile(PLL0STAT) & PLL0STAT_PLOCK == 0 {}

        // CCLK divider before connecting (so CCLK doesn't briefly run at 288 MHz).
        ptr::write_volatile(CCLKCFG, CCLKCFG_DIV3);

        // Connect PLL0 to CCLK.
        ptr::write_volatile(PLL0CON, PLL0CON_PLLE | PLL0CON_PLLC);
        feed_pll0();
    }
}

fn setup_uart0_115200() {
    unsafe {
        ptr::write_volatile(PCONP, ptr::read_volatile(PCONP) | PCONP_PCUART0);

        // UART0 PCLK = CCLK / 1 = 96 MHz.
        let mut pclksel = ptr::read_volatile(PCLKSEL0);
        pclksel = (pclksel & !PCLKSEL0_UART0_MASK) | PCLKSEL0_UART0_CCLK_DIV1;
        ptr::write_volatile(PCLKSEL0, pclksel);

        // Pin mux: P0.2 → TXD0, P0.3 → RXD0.
        let mut pinsel = ptr::read_volatile(PINSEL0);
        pinsel = (pinsel & !PINSEL0_P0_2_P0_3_MASK) | PINSEL0_P0_2_TXD0 | PINSEL0_P0_3_RXD0;
        ptr::write_volatile(PINSEL0, pinsel);

        // 115200 8N1.
        ptr::write_volatile(UART0_LCR, LCR_DLAB);
        ptr::write_volatile(UART0_DLL, BAUD_DLL_115200_AT_96MHZ);
        ptr::write_volatile(UART0_DLM, BAUD_DLM_115200_AT_96MHZ);
        ptr::write_volatile(UART0_LCR, LCR_8N1);
        ptr::write_volatile(UART0_FCR, FCR_FIFO_ENABLE_AND_RESET);
        ptr::write_volatile(UART0_IER, 0);
    }
}

pub struct Lpc1768;

impl super::Board for Lpc1768 {
    /// CCLK after `init()`: 96 MHz.
    ///
    /// Sourced from the mbed's 12 MHz crystal via PLL0 (M=12, N=1, /3).
    /// Crystal-stable, ~50 ppm.
    const CCLK_HZ: u32 = 96_000_000;

    type Tx = UartTx;
    type Rx = Uart0Rx;
    type Timeout = Timer0;

    fn init() {
        setup_clock_96mhz();
        setup_uart0_115200();
    }
}

#[derive(Default)]
pub struct UartTx;

impl super::UartTx for UartTx {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while ptr::read_volatile(UART0_LSR) & LSR_THRE == 0 {}
            ptr::write_volatile(UART0_THR, byte as u32);
        }
    }
}

/// Timer0 as a one-shot match interrupt.
///
/// PCLK_TIMER0 is set to CCLK/1 (96 MHz) so the Timer Counter increments
/// once per CCLK cycle and MR0 holds the deadline in CCLK cycles directly.
pub struct Timer0;

impl super::TimeoutTimer for Timer0 {
    fn init() {
        unsafe {
            // Power Timer0.
            let pconp = ptr::read_volatile(PCONP);
            ptr::write_volatile(PCONP, pconp | PCONP_PCTIM0);

            // PCLK_TIMER0 = CCLK / 1, so 1 TC tick == 1 CCLK cycle.
            let mut pclksel = ptr::read_volatile(PCLKSEL0);
            pclksel = (pclksel & !PCLKSEL0_TIMER0_MASK) | PCLKSEL0_TIMER0_CCLK_DIV1;
            ptr::write_volatile(PCLKSEL0, pclksel);

            // Leave disarmed.
            ptr::write_volatile(T0_TCR, 0);
            ptr::write_volatile(T0_PR, 0); // no prescale
            ptr::write_volatile(T0_MCR, 0);
            ptr::write_volatile(T0_IR, IR_MR0);
        }
        NVIC::unpend(Interrupt::TIMER0);
        // SAFETY: enabling this interrupt only signals the timeout path,
        // which is required for the user-program sandbox to work.
        unsafe { NVIC::unmask(Interrupt::TIMER0) };
    }

    fn arm_in_cycles(cycles: u32) {
        unsafe {
            // Stop, reset counter to 0, set match value, enable match
            // interrupt, clear any prior IR, then run.
            ptr::write_volatile(T0_TCR, 0);
            ptr::write_volatile(T0_TCR, TCR_RESET);
            ptr::write_volatile(T0_TCR, 0);
            ptr::write_volatile(T0_MR0, cycles.max(1));
            ptr::write_volatile(T0_MCR, MCR_MR0I);
            ptr::write_volatile(T0_IR, IR_MR0);
            ptr::write_volatile(T0_TCR, TCR_ENABLE);
        }
    }

    fn disarm() {
        unsafe {
            ptr::write_volatile(T0_TCR, 0);
            ptr::write_volatile(T0_MCR, 0);
            ptr::write_volatile(T0_IR, IR_MR0);
        }
        NVIC::unpend(Interrupt::TIMER0);
    }
}

/// Interrupt handler for Timer0 (NVIC IRQ 1).
///
/// Routed into the local `lpc176x` PAC's vector table by the `#[interrupt]`
/// attribute, which resolves the variant against `lpc176x::Interrupt`.
#[interrupt]
fn TIMER0() {
    unsafe {
        ptr::write_volatile(T0_IR, IR_MR0);
    }
    crate::user::on_timeout_match();
}

/// UART0 receive side.
///
/// The ISR drains the RX FIFO into [`RX_RING`] (SPSC: ISR producer,
/// main-thread consumer); [`read_byte`] pops one without blocking.
/// Overruns drop bytes — the host retransmits on CRC failure.
pub struct Uart0Rx;

static RX_RING: SpscRing<512> = SpscRing::new();

impl super::UartRx for Uart0Rx {
    fn init() {
        unsafe {
            // Enable the Receive Data Available + Character Time-out
            // interrupt source. (RDA fires once the FIFO trigger level
            // is met; CTI fires for stragglers when the line goes idle.)
            ptr::write_volatile(UART0_IER, IER_RBR);
        }
        NVIC::unpend(Interrupt::UART0);
        // SAFETY: enabling this interrupt only routes UART RX bytes into
        // the SPSC ring; it doesn't interfere with critical sections.
        unsafe { NVIC::unmask(Interrupt::UART0) };
    }

    fn read_byte() -> Option<u8> {
        RX_RING.pop()
    }
}

#[interrupt]
fn UART0() {
    unsafe {
        // IIR.INTID identifies the source; drain the FIFO while LSR.RDR
        // is set. We service both RDA and CTI here.
        loop {
            let iir = ptr::read_volatile(UART0_IIR);
            let intid = iir & IIR_INTID_MASK;
            if intid != IIR_INTID_RDA && intid != IIR_INTID_CTI {
                break;
            }
            while ptr::read_volatile(UART0_LSR) & LSR_RDR != 0 {
                let byte = ptr::read_volatile(UART0_RBR) as u8;
                RX_RING.push(byte);
            }
        }
    }
}
