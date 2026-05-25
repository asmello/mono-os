/* Linker layout for the lm3s6965evb QEMU machine (Cortex-M3).
 *
 * The 64 KiB SRAM splits 32 / 32 kernel/user. The declared RAM region
 * below covers only the kernel half. User-side SRAM lives at
 * 0x2000_8000 .. 0x2001_0000 and is reached only through raw pointers
 * from the kernel; the MPU then carves it into three power-of-two
 * regions (8 KiB text + 8 KiB data + 16 KiB stack) before unprivileged
 * user code runs.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 256K
  RAM   : ORIGIN = 0x20000000, LENGTH = 32K
}
