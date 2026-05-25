/* Linker layout for the mbed NXP LPC1769/68 (Cortex-M3, 512K flash, 64K SRAM).
 *
 * The 64 KiB SRAM splits across three banks:
 *
 *     0x1000_0000  32 KiB CPU-local SRAM   — kernel only (declared below)
 *     0x2007_C000  16 KiB AHB SRAM bank 0  — user.text + user.data
 *     0x2008_0000  16 KiB AHB SRAM bank 1  — user.stack
 *
 * The kernel-side RAM region covers only the local SRAM, so cortex-m-rt
 * places the initial MSP at 0x1000_8000 (top of local SRAM) and the
 * linker errors if .bss + stack overflow it. AHB SRAM is reached only
 * through raw pointers from the kernel; the MPU then carves it into three
 * power-of-two regions (text/data/stack) before unprivileged user code
 * runs. See `kernel/src/user.rs` and `kernel/src/mpu.rs` for the user-side
 * layout and the MPU sizing.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM   : ORIGIN = 0x10000000, LENGTH = 32K
}
