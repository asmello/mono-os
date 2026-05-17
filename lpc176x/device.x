/* Weak aliases for the LPC176x device-specific interrupt handlers.
 * cortex-m-rt's link.x INCLUDEs this file when the `device` feature is on.
 * Each PROVIDE lets user code override the corresponding handler by
 * defining a function with that name (e.g. via `#[interrupt] fn TIMER0`).
 */
PROVIDE(WDT          = DefaultHandler);
PROVIDE(TIMER0       = DefaultHandler);
PROVIDE(TIMER1       = DefaultHandler);
PROVIDE(TIMER2       = DefaultHandler);
PROVIDE(TIMER3       = DefaultHandler);
PROVIDE(UART0        = DefaultHandler);
PROVIDE(UART1        = DefaultHandler);
PROVIDE(UART2        = DefaultHandler);
PROVIDE(UART3        = DefaultHandler);
PROVIDE(PWM1         = DefaultHandler);
PROVIDE(I2C0         = DefaultHandler);
PROVIDE(I2C1         = DefaultHandler);
PROVIDE(I2C2         = DefaultHandler);
PROVIDE(SPI          = DefaultHandler);
PROVIDE(SSP0         = DefaultHandler);
PROVIDE(SSP1         = DefaultHandler);
PROVIDE(PLL0         = DefaultHandler);
PROVIDE(RTC          = DefaultHandler);
PROVIDE(EINT0        = DefaultHandler);
PROVIDE(EINT1        = DefaultHandler);
PROVIDE(EINT2        = DefaultHandler);
PROVIDE(EINT3        = DefaultHandler);
PROVIDE(ADC          = DefaultHandler);
PROVIDE(BOD          = DefaultHandler);
PROVIDE(USB          = DefaultHandler);
PROVIDE(CAN          = DefaultHandler);
PROVIDE(DMA          = DefaultHandler);
PROVIDE(I2S          = DefaultHandler);
PROVIDE(ENET         = DefaultHandler);
PROVIDE(RIT          = DefaultHandler);
PROVIDE(MCPWM        = DefaultHandler);
PROVIDE(QEI          = DefaultHandler);
PROVIDE(PLL1         = DefaultHandler);
PROVIDE(USBActivity  = DefaultHandler);
PROVIDE(CANActivity  = DefaultHandler);
