/* Linker layout for the lm3s6965evb QEMU machine (Cortex-M3). */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 256K
  RAM   : ORIGIN = 0x20000000, LENGTH = 64K
}
