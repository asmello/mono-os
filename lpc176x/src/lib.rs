//! Interrupt bindings for the NXP LPC176x ARM Cortex-M3 family.
//!
//! Shaped like a peripheral access crate (PAC) but covers only what
//! cortex-m-rt's `#[interrupt]` attribute needs: the `Interrupt` enum
//! (also re-exported in lowercase as `interrupt` so the macro can resolve
//! `interrupt::TIMER0`), the `__INTERRUPTS` vector table, and a weak-alias
//! `device.x` linker script. Register access lives in the kernel; the SVD
//! crates that cover the full LPC1768 register set are large and not
//! needed for this project.
//!
//! Reference: UM10360 §6.5.7 (NVIC interrupt sources).

#![no_std]
#![allow(non_camel_case_types)]

pub use self::Interrupt as interrupt;
pub use cortex_m_rt::interrupt;

/// Enumeration of LPC176x device-specific interrupts (NVIC IRQ numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Interrupt {
    WDT = 0,
    TIMER0 = 1,
    TIMER1 = 2,
    TIMER2 = 3,
    TIMER3 = 4,
    UART0 = 5,
    UART1 = 6,
    UART2 = 7,
    UART3 = 8,
    PWM1 = 9,
    I2C0 = 10,
    I2C1 = 11,
    I2C2 = 12,
    SPI = 13,
    SSP0 = 14,
    SSP1 = 15,
    PLL0 = 16,
    RTC = 17,
    EINT0 = 18,
    EINT1 = 19,
    EINT2 = 20,
    EINT3 = 21,
    ADC = 22,
    BOD = 23,
    USB = 24,
    CAN = 25,
    DMA = 26,
    I2S = 27,
    ENET = 28,
    RIT = 29,
    MCPWM = 30,
    QEI = 31,
    PLL1 = 32,
    USBActivity = 33,
    CANActivity = 34,
}

unsafe impl cortex_m::interrupt::InterruptNumber for Interrupt {
    #[inline(always)]
    fn number(self) -> u16 {
        self as u16
    }
}

/// Single slot in `.vector_table.interrupts`.
///
/// Same shape as cortex-m-rt's private `Vector` union; we mirror it here
/// because the fields on theirs are `doc(hidden)` and so cannot be
/// constructed from outside the crate.
#[doc(hidden)]
#[repr(C)]
pub union Vector {
    _handler: unsafe extern "C" fn(),
    _reserved: u32,
}

unsafe extern "C" {
    fn WDT();
    fn TIMER0();
    fn TIMER1();
    fn TIMER2();
    fn TIMER3();
    fn UART0();
    fn UART1();
    fn UART2();
    fn UART3();
    fn PWM1();
    fn I2C0();
    fn I2C1();
    fn I2C2();
    fn SPI();
    fn SSP0();
    fn SSP1();
    fn PLL0();
    fn RTC();
    fn EINT0();
    fn EINT1();
    fn EINT2();
    fn EINT3();
    fn ADC();
    fn BOD();
    fn USB();
    fn CAN();
    fn DMA();
    fn I2S();
    fn ENET();
    fn RIT();
    fn MCPWM();
    fn QEI();
    fn PLL1();
    fn USBActivity();
    fn CANActivity();
}

#[doc(hidden)]
#[unsafe(link_section = ".vector_table.interrupts")]
#[unsafe(no_mangle)]
pub static __INTERRUPTS: [Vector; 35] = [
    Vector { _handler: WDT },
    Vector { _handler: TIMER0 },
    Vector { _handler: TIMER1 },
    Vector { _handler: TIMER2 },
    Vector { _handler: TIMER3 },
    Vector { _handler: UART0 },
    Vector { _handler: UART1 },
    Vector { _handler: UART2 },
    Vector { _handler: UART3 },
    Vector { _handler: PWM1 },
    Vector { _handler: I2C0 },
    Vector { _handler: I2C1 },
    Vector { _handler: I2C2 },
    Vector { _handler: SPI },
    Vector { _handler: SSP0 },
    Vector { _handler: SSP1 },
    Vector { _handler: PLL0 },
    Vector { _handler: RTC },
    Vector { _handler: EINT0 },
    Vector { _handler: EINT1 },
    Vector { _handler: EINT2 },
    Vector { _handler: EINT3 },
    Vector { _handler: ADC },
    Vector { _handler: BOD },
    Vector { _handler: USB },
    Vector { _handler: CAN },
    Vector { _handler: DMA },
    Vector { _handler: I2S },
    Vector { _handler: ENET },
    Vector { _handler: RIT },
    Vector { _handler: MCPWM },
    Vector { _handler: QEI },
    Vector { _handler: PLL1 },
    Vector {
        _handler: USBActivity,
    },
    Vector {
        _handler: CANActivity,
    },
];
