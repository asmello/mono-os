//! Board driver for the Stellaris LM3S6965 (as emulated by QEMU's
//! `lm3s6965evb` machine).
//!
//! Provides a blocking TX-only UART0 driver and a one-shot deadline timer
//! built on General-Purpose Timer Module 0 (GPTM0, Timer A). Register
//! layout per the LM3S6965 data sheet. QEMU emulates the UART as
//! always-ready, but the TXFF poll is still useful on real hardware and
//! costs one MMIO read per byte.

use core::ptr;
use lm3s6965::interrupt;

const UART0_BASE: usize = 0x4000_C000;
const UARTDR: *mut u32 = UART0_BASE as *mut u32;
const UARTFR: *const u32 = (UART0_BASE + 0x018) as *const u32;
const UARTIM: *mut u32 = (UART0_BASE + 0x038) as *mut u32;
const UARTMIS: *const u32 = (UART0_BASE + 0x040) as *const u32;
const UARTICR: *mut u32 = (UART0_BASE + 0x044) as *mut u32;
const UARTFR_TXFF: u32 = 1 << 5;
const UARTFR_RXFE: u32 = 1 << 4;
const UART_IM_RXIM: u32 = 1 << 4; // RX FIFO trigger interrupt enable
const UART_IM_RTIM: u32 = 1 << 6; // RX timeout interrupt enable
const IRQ_UART0: u32 = 5;

// System control (LM3S6965 §5.4).
const SYSCTL_RCGC1: *mut u32 = 0x400F_E104 as *mut u32;
const SYSCTL_RCGC1_TIMER0: u32 = 1 << 16;

// GPTM0 (LM3S6965 §10.5, base 0x4003_0000).
const GPTM0_BASE: usize = 0x4003_0000;
const GPTMCFG: *mut u32 = GPTM0_BASE as *mut u32;
const GPTMTAMR: *mut u32 = (GPTM0_BASE + 0x004) as *mut u32;
const GPTMCTL: *mut u32 = (GPTM0_BASE + 0x00C) as *mut u32;
const GPTMIMR: *mut u32 = (GPTM0_BASE + 0x018) as *mut u32;
const GPTMICR: *mut u32 = (GPTM0_BASE + 0x024) as *mut u32;
const GPTMTAILR: *mut u32 = (GPTM0_BASE + 0x028) as *mut u32;

const GPTMCFG_32BIT: u32 = 0;
const GPTMTAMR_ONESHOT: u32 = 0x1;
const GPTMCTL_TAEN: u32 = 1 << 0;
const GPTM_TATOIM: u32 = 1 << 0; // shared bit position in IMR/RIS/ICR.

// NVIC set-enable register 0 (covers IRQs 0..31). GPT0A is IRQ 19.
const NVIC_ISER0: *mut u32 = 0xE000_E100 as *mut u32;
const NVIC_ICPR0: *mut u32 = 0xE000_E280 as *mut u32;
const IRQ_GPT0A: u32 = 19;

pub struct Lm3s6965evb;

impl super::Board for Lm3s6965evb {
    /// Approximate emulated core clock under QEMU's `lm3s6965evb` machine.
    /// The chip's real reset clock is PIOSC at 6 MHz, but QEMU's stellaris
    /// timing is not authoritative — this is a ballpark value chosen to give
    /// a roughly visible tick rate.
    const CCLK_HZ: u32 = 12_000_000;

    type Uart = Uart;
    type Rx = Uart0Rx;
    type Timeout = Gpt0;

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

/// GPTM0 Timer A as a one-shot match interrupt. `arm_in_cycles` loads the
/// requested cycle count into GPTMTAILR and enables the timer; when it
/// counts down to zero the timer raises TATOIM, NVIC line 19 fires, and
/// the handler [`Gpt0aHandler`] acks the peripheral and signals the
/// kernel via [`crate::timeout::mark_fired`].
pub struct Gpt0;

impl super::TimeoutTimer for Gpt0 {
    fn init() {
        unsafe {
            // Clock the timer (no-op under QEMU, real on hardware).
            let rcgc1 = ptr::read_volatile(SYSCTL_RCGC1);
            ptr::write_volatile(SYSCTL_RCGC1, rcgc1 | SYSCTL_RCGC1_TIMER0);

            // Leave disarmed; tests/kernel arm on demand.
            ptr::write_volatile(GPTMCTL, 0);
            ptr::write_volatile(GPTMCFG, GPTMCFG_32BIT);
            ptr::write_volatile(GPTMTAMR, GPTMTAMR_ONESHOT);
            ptr::write_volatile(GPTMIMR, 0);
            ptr::write_volatile(GPTMICR, GPTM_TATOIM);

            // NVIC: clear any stale pending bit, then enable IRQ 19.
            ptr::write_volatile(NVIC_ICPR0, 1 << IRQ_GPT0A);
            ptr::write_volatile(NVIC_ISER0, 1 << IRQ_GPT0A);
        }
    }

    fn arm_in_cycles(cycles: u32) {
        unsafe {
            ptr::write_volatile(GPTMCTL, 0);
            ptr::write_volatile(GPTMICR, GPTM_TATOIM);
            // GPTMTAILR is the start value; the timer counts down to 0.
            // A 0 reload would never trigger — clamp to 1.
            ptr::write_volatile(GPTMTAILR, cycles.max(1));
            ptr::write_volatile(GPTMIMR, GPTM_TATOIM);
            ptr::write_volatile(GPTMCTL, GPTMCTL_TAEN);
        }
    }

    fn disarm() {
        unsafe {
            ptr::write_volatile(GPTMCTL, 0);
            ptr::write_volatile(GPTMIMR, 0);
            ptr::write_volatile(GPTMICR, GPTM_TATOIM);
            ptr::write_volatile(NVIC_ICPR0, 1 << IRQ_GPT0A);
        }
    }
}

/// Interrupt handler for GPT0 Timer A timeout (NVIC IRQ 19). Routed into
/// the lm3s6965 PAC's vector table by the `#[interrupt]` attribute, which
/// resolves the variant against `lm3s6965::Interrupt`.
#[interrupt]
fn TIMER_0A() {
    unsafe {
        ptr::write_volatile(GPTMICR, GPTM_TATOIM);
    }
    crate::user::on_timeout_match();
}

/// UART0 receive side. QEMU's emulated PL011 raises the RX interrupt as
/// bytes arrive on the `-serial` backend. The ISR drains UARTDR into a
/// 512-byte ring; [`read_byte`] pops one non-blockingly.
pub struct Uart0Rx;

impl super::UartRx for Uart0Rx {
    fn init() {
        unsafe {
            // Enable RX FIFO trigger + RX timeout interrupts.
            ptr::write_volatile(UARTICR, UART_IM_RXIM | UART_IM_RTIM);
            ptr::write_volatile(UARTIM, UART_IM_RXIM | UART_IM_RTIM);
            ptr::write_volatile(NVIC_ICPR0, 1 << IRQ_UART0);
            ptr::write_volatile(NVIC_ISER0, 1 << IRQ_UART0);
        }
    }

    fn read_byte() -> Option<u8> {
        rx_ring::pop()
    }
}

#[interrupt]
fn UART0() {
    unsafe {
        // Ack any pending RX-source bits, then drain while FIFO non-empty.
        ptr::write_volatile(UARTICR, UART_IM_RXIM | UART_IM_RTIM);
        while ptr::read_volatile(UARTFR) & UARTFR_RXFE == 0 {
            let byte = ptr::read_volatile(UARTDR) as u8;
            rx_ring::push(byte);
        }
        // Re-ack so we don't take a spurious re-entry on stale MIS.
        let _ = ptr::read_volatile(UARTMIS);
        ptr::write_volatile(UARTICR, UART_IM_RXIM | UART_IM_RTIM);
    }
}

mod rx_ring {
    //! SPSC byte ring: producer = UART0 ISR, consumer = main thread.
    //! Identical shape to the LPC1768 ring; see that file for notes.

    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicUsize, Ordering};

    const SIZE: usize = 512;
    const MASK: usize = SIZE - 1;

    struct Buf(UnsafeCell<[u8; SIZE]>);
    unsafe impl Sync for Buf {}

    static BUF: Buf = Buf(UnsafeCell::new([0; SIZE]));
    static HEAD: AtomicUsize = AtomicUsize::new(0);
    static TAIL: AtomicUsize = AtomicUsize::new(0);

    pub fn push(byte: u8) {
        let head = HEAD.load(Ordering::Relaxed);
        let tail = TAIL.load(Ordering::Acquire);
        let next = (head + 1) & MASK;
        if next == tail {
            return;
        }
        unsafe { (*BUF.0.get())[head] = byte };
        HEAD.store(next, Ordering::Release);
    }

    pub fn pop() -> Option<u8> {
        let tail = TAIL.load(Ordering::Relaxed);
        let head = HEAD.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let byte = unsafe { (*BUF.0.get())[tail] };
        TAIL.store((tail + 1) & MASK, Ordering::Release);
        Some(byte)
    }
}
