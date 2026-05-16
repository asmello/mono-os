/* Linker layout for the mbed NXP LPC1768 (Cortex-M3, 512K flash, 32K main SRAM). */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM   : ORIGIN = 0x10000000, LENGTH = 32K
}
