/* Linker layout for the lm3s6965evb QEMU machine (Cortex-M3).
 *
 * The 64 KiB SRAM is split kernel/user. The declared RAM region below
 * covers only the kernel half (48 KiB; QEMU has more headroom than the
 * LPC1768, so the kernel side absorbs the slack). User-side SRAM lives at
 * 0x2000_C000 .. 0x2001_0000 and is reached only through raw pointers from
 * the kernel; the MPU carves it into power-of-two regions before
 * unprivileged user code runs. Keeping the same user split as the LPC1768
 * means the user-side addresses are identical at the offset level.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 256K
  RAM   : ORIGIN = 0x20000000, LENGTH = 48K
}
