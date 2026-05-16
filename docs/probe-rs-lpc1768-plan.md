# Deferred: probe-rs LPC1768 chip support

## Context

probe-rs 0.31 ships ~4,400 chip targets but **no LPC1768**. NXP's older LPC176x family was never ported into the probe-rs target database — coverage stops around LPC11xx / LPC54xxx / LPC55xx and the newer i.MX RT line. Generic ARMv7-M is offered but without a flash algorithm, so `probe-rs run` can't write flash with the generic target alone.

We pivoted to OpenOCD for the LPC1768 flash path (commit ↓). This file captures the deferred probe-rs work so we can pick it up cleanly when we want a unified flasher across all future boards.

When this matters again: once we add STM32F429 / nRF52840 and find ourselves with two flasher binaries (openocd for LPC1768, probe-rs for everything else). At that point a probe-rs LPC1768 chip definition is worth the investment.

## What's needed

A probe-rs chip definition is a YAML file with two parts:

1. **Chip description**: name, manufacturer, cores, memory map, sector layout. Mostly mechanical.
2. **Flash algorithm**: a self-contained Cortex-M binary that probe-rs loads into the target's RAM and calls at known offsets (`Init`, `EraseSector`, `ProgramPage`, `EraseAll`, `UnInit`, `Verify`). The algorithm runs *on the chip*; probe-rs orchestrates via SWD/JTAG.

Two ways to produce (2):

- **Adapt a CMSIS-Pack**. NXP publishes `Keil.LPC1700_DFP` which contains FlashOS-format flash algorithms. probe-rs's `target-gen` tool converts CMSIS Packs to its YAML format. Cleanest if the pack's algorithm still builds. The pack also includes the chip description, so most of (1) comes for free.
- **Write from scratch** in Rust using the [`flash-algorithm`](https://github.com/probe-rs/flash-algorithm) crate template. Implement against the LPC1768's **IAP** (In-Application Programming) ROM API at `0x1FFF_1FF1`, documented in UM10360 chapter 32. This is the most legible approach for someone learning probe-rs internals.

## Sketch — from-scratch approach (IAP)

### Crate layout

```
probe-rs-targets/
└── lpc1768-flash-algorithm/
    ├── Cargo.toml          # cdylib? no — binary. Linker script for placement in RAM.
    ├── build.rs            # provides memory.x targeting RAM
    ├── memory.x            # FLASH section = 0 (we don't link anything to flash);
    │                       # RAM section = where probe-rs will load us
    ├── src/lib.rs          # algorithm! { ... } macro from flash-algorithm crate
    └── README.md
```

### LPC1768 IAP basics (UM10360 §32.5)

- Function pointer: `0x1FFF_1FF1` (Thumb mode bit set).
- Calling convention: `fn(command: *const u32, result: *mut u32)`, both 5-word arrays.
- Commands we need:
  - 50: Prepare sectors for write.
  - 51: Copy RAM to flash. Sizes: 256, 512, 1024, or 4096 bytes.
  - 52: Erase sectors.
  - 53: Blank check sectors.
  - 54: Read part ID.
- Return codes: 0 = CMD_SUCCESS. Many others (INVALID_COMMAND, BUSY, SECTOR_NOT_PREPARED_FOR_WRITE_OPERATION, COMPARE_ERROR, etc.).
- **Sector unlock pattern**: every flash-mutating op needs a fresh "prepare" call before it (sectors lock again after each modify).
- **Vector-table checksum** (UM10360 §32.3.1): word at offset `0x1C` must be the two's complement of the sum of the first seven vector entries. probe-rs's flash algorithm should patch this before/during `ProgramPage` if the first page is being written. (alternatively patch at build time in our kernel `build.rs` and skip this here.)

### Sector map

| Range            | Sector indices | Sector size |
|------------------|----------------|-------------|
| `0x00000`–`0x0FFFF` | 0–15           | 4 KiB       |
| `0x10000`–`0x7FFFF` | 16–29          | 32 KiB      |

Total: 16·4 + 14·32 = 64 + 448 = 512 KiB ✓

### algorithm! invocation (sketch)

```rust
#![no_std]
#![no_main]
use flash_algorithm::*;

struct LpcAlgo;

algorithm!(LpcAlgo, {
    flash_address: 0x0000_0000,
    flash_size:    0x0008_0000,           // 512 KiB
    page_size:     1024,                  // IAP supports 256/512/1024/4096
    empty_value:   0xFF,
    sectors: [
        { size: 0x1000, address: 0x0000_0000 },   // 16 × 4 KiB
        { size: 0x8000, address: 0x0001_0000 },   // 14 × 32 KiB
    ],
});

impl FlashAlgorithm for LpcAlgo {
    fn new(_addr: u32, _clock: u32, _function: Function) -> Result<Self, ErrorCode> {
        // No init needed; IAP works from reset state.
        Ok(LpcAlgo)
    }
    fn erase_sector(&mut self, addr: u32) -> Result<(), ErrorCode> { /* prepare + erase */ }
    fn program_page(&mut self, addr: u32, data: &[u8]) -> Result<(), ErrorCode> { /* prepare + copy-ram-to-flash */ }
    fn erase_all(&mut self) -> Result<(), ErrorCode> { /* iterate sectors */ }
}
```

The IAP wrapper:

```rust
const IAP: extern "C" fn(*const u32, *mut u32) =
    unsafe { core::mem::transmute(0x1FFF_1FF1usize) };

fn iap(cmd: &[u32; 5]) -> [u32; 5] {
    let mut result = [0u32; 5];
    IAP(cmd.as_ptr(), result.as_mut_ptr());
    result
}
```

### Building the YAML

Once the algorithm crate compiles to `thumbv7m-none-eabi`, `target-gen` extracts the `.text`/`.data` sections, packages them as a base64 / hex blob, and embeds them in the chip YAML alongside the chip description (memory map + sectors). Output:

```
probe-rs-targets/LPC1768.yaml
```

Usage:

```
probe-rs run --chip-description-path probe-rs-targets/LPC1768.yaml --chip LPC1768 target/.../kernel
```

## When to do this

Defer until at least one of:

1. We hit two-flasher friction (openocd for LPC, probe-rs for others).
2. We need RTT on the LPC1768 (probe-rs has a fancier RTT story than openocd's `monitor rtt`).
3. We want to contribute the chip definition upstream to probe-rs.

## References (for future me)

- LPC1768 IAP: UM10360 chapter 32 (`UM10360.pdf` in `~/Downloads/`).
- LPC1768 vector checksum: UM10360 §32.3.1.
- probe-rs flash-algorithm crate template: <https://github.com/probe-rs/flash-algorithm>.
- probe-rs target-gen: <https://github.com/probe-rs/probe-rs/tree/master/target-gen>.
- CMSIS-Pack for LPC1700 series: NXP publishes `Keil.LPC1700_DFP`; downloadable from <https://www.keil.com/dd2/Pack/>.
- Errata that may matter: `ES_LPC176X.pdf` — check before declaring done.
