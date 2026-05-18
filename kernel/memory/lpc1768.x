/* Linker layout for the mbed NXP LPC1768 (Cortex-M3, 512K flash, 32K main SRAM).
 *
 * The 32 KiB main SRAM splits into a kernel half and a user half. The kernel
 * RAM region declared below covers only the kernel half so cortex-m-rt places
 * the initial MSP at 0x1000_4000 (top of kernel half) and the linker errors
 * if .bss + stack overflow into user space. The user half lives at
 * 0x1000_4000 .. 0x1000_8000 and is reached only through raw pointers from
 * the kernel; the MPU then carves it into three power-of-two regions
 * (text/data/stack) before unprivileged user code runs.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM   : ORIGIN = 0x10000000, LENGTH = 16K
}
