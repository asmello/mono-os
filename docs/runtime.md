# mono-os runtime architecture

This document is the post-M4 picture: what's on the device, what the host
ships to it, and the contract between the two. The original Phil-Opp port
plan that the project grew out of is preserved at
[`docs/history/arm-port.md`](history/arm-port.md).

## What this is

mono-os runs **one precompiled user program at a time** under hardware
memory protection with a fixed deadline, and reports cycle-accurate
timing back to the host. The motivation is deterministic benchmarking:
removing context switching, demand paging, and dynamic scheduling lets
small algorithms be measured with cycle-stable timing.

The kernel is bare-metal Rust on ARMv7-M (Cortex-M3). User programs are
flat binaries linked against a small `userlib` and shipped over UART.
There is no scheduler, no heap, no filesystem, no second user program.

Supported boards:

| Board           | Use                                  |
|-----------------|--------------------------------------|
| `lm3s6965evb`   | QEMU emulation — the fast dev loop.  |
| LPC1768 (mbed)  | Real hardware. 96 MHz, 32 KiB SRAM.  |

## Memory layout

The kernel splits SRAM in half: kernel side has the MSP, .data, and .bss;
user side is carved into three power-of-2 MPU regions:

```
0x1000_0000 ┬─────────────────────────┐
            │ kernel .data / .bss      │
            │ + MSP (kernel stack)     │ 16 KiB  kernel-private
0x1000_4000 ┼─────────────────────────┤
            │ user .text (R+X)         │  4 KiB  MPU region 0 (unpriv RX)
0x1000_5000 ┼─────────────────────────┤
            │ user .data / .bss        │  4 KiB  MPU region 1 (unpriv RW, XN)
0x1000_6000 ┼─────────────────────────┤
            │ user stack (PSP)         │  8 KiB  MPU region 2 (unpriv RW, XN)
0x1000_8000 ┴─────────────────────────┘
```

(LPC1768 addresses. On `lm3s6965evb` the user split lives at
`0x2000_C000`..`0x2001_0000`; the kernel side gets the slack since QEMU's
SRAM is 64 KiB.)

`PRIVDEFENA = 1` gives privileged (kernel) code a flat background view of
the whole address space, so kernel-visible regions don't need to be
enumerated. Unprivileged thread-mode code sees **only** what regions 0–2
grant; everything else faults MemManage.

The regions are W^X-clean: user_text is RX-no-W, user_data and
user_stack are RW-no-X. A user program that tries to execute from its
data region or write to flash gets a MemManage fault and the run is
aborted.

The first 12 bytes of user_text are reserved for a kernel-installed
privilege-drop trampoline (see [Privilege transition](#privilege-transition)).
User code starts at `USER_TEXT_BASE + 12`.

## Executable format — "monoexec v0"

Flat binary at a fixed load address. No relocation, no PIC, no dynamic
linker. 32-byte header, then `text_size` bytes of code+rodata, then
`data_size` bytes of `.data` init image:

```rust
#[repr(C)]
struct ExecHeader {
    magic:        [u8; 4],   // b"MOXE"
    version:      u16,       // 0
    flags:        u16,       // reserved, must be 0
    entry_offset: u32,       // bytes from start of .text to first instruction
    text_size:    u32,       // bytes to copy to USER_TEXT_BASE + 12
    data_size:    u32,       // bytes to copy to USER_DATA_BASE
    bss_size:     u32,       // bytes the kernel zeros after .data
    timeout_ms:   u32,       // hard cap; kernel aborts past this
    reserved:     u32,       // 0
}
```

Sizes must satisfy `text_size ≤ 4 KiB - 12` and
`data_size + bss_size ≤ 4 KiB`. The kernel rejects anything else with a
NAK before execution.

The header is an **internal contract** between the kernel and the
compile-side `userlib`. It changes freely in lockstep — there is no
backwards-compatibility surface for external `.bin`s. `version` exists
only as a sanity gate against accidental mismatches.

## monolink wire protocol

Stop-and-wait framing over UART, host-initiated. One frame at a time, no
flow control beyond ACK/NAK.

```
host → device:   START_LOAD frame  →  device replies ACK or NAK
host → device:   N CHUNK frames    →  device ACKs each
host → device:   EOF frame         →  device ACKs, then runs the program
host                                    device runs the program
                                  ←  RESULT frame  (status + exit + cycles)
```

Frame format on the wire (little-endian throughout):

```
+-------+------+--------+------------------+----------+
|  SOF  | TYPE |  LEN   |     PAYLOAD      |  CRC32   |
| 0x55  |  u8  |  u16   |   LEN bytes      |   u32    |
+-------+------+--------+------------------+----------+
```

CRC32 is IEEE 802.3 (zlib flavor) computed over `TYPE || LEN || PAYLOAD`.

Frame types:

| TYPE | Direction | Name         | Payload                          |
|------|-----------|--------------|----------------------------------|
| 0x01 | host → dev | START_LOAD   | 32-byte `ExecHeader`             |
| 0x02 | host → dev | CHUNK        | up to 256 bytes of body          |
| 0x03 | host → dev | EOF          | empty                            |
| 0x10 | dev → host | ACK          | empty                            |
| 0x11 | dev → host | NAK          | 1-byte `NakReason`               |
| 0x20 | dev → host | RESULT       | 20-byte result                   |

NAK reasons (`monolink-proto::NakReason`) cover framing errors (oversized
frame, CRC mismatch, unknown type), session-sequencing errors (chunk
before start_load), and loader errors (bad header magic, oversized text
or data). The kernel never emits raw text mid-session — that would
interleave with framed bytes — so the host tool renders the reason byte
into the user-facing message.

RESULT payload:

```rust
#[repr(C)]
struct ResultFrame {
    status:    u8,   // 0=OK, 1=TIMEOUT, 2=MEMFAULT, 3=BUSFAULT,
                     // 4=USAGEFAULT, 5=LOAD_ERROR
    _pad:      [u8; 3],
    exit_code: u32,  // user-supplied on OK; reason-specific on faults
    cycles:    u64,  // CPU cycles between launch and termination
    cclk_hz:   u32,  // CCLK at which those cycles were counted
}
```

The wire format lives in the `monolink-proto` crate, shared verbatim
between the kernel and the host tool (`tools/monolink`).

## Privilege transition

Kernel boots into privileged thread mode on MSP (the cortex-m-rt
default). Launching a user program is *not* a single "drop privilege and
branch" — ARMv7-M has no such instruction in thread mode. The sequence:

1. Configure MPU regions 0–2 for the user, enable MPU with
   `PRIVDEFENA = 1`.
2. Pause SysTick, arm the per-run timeout timer, snapshot
   `DWT.CYCCNT`.
3. Copy a four-instruction **privilege-drop trampoline**
   (`movs r3,#3; msr control,r3; isb; bx r0`) into the first 12 bytes of
   user_text. Region 0 is unpriv-RX, so the post-ISB fetch lands
   somewhere unprivileged code is permitted to execute. (The same
   sequence inline in kernel flash would fault: post-ISB the very next
   fetch is at the new privilege level, and kernel flash is not in the
   unprivileged MPU view.)
4. Set PSP to top of user_stack region. Switch SP_active to PSP
   without dropping privilege yet.
5. Branch to the trampoline. It drops privilege via `msr control` and
   then BXes to the user's true entry (passed in R0).

From here on user code can only re-enter the kernel through three doors:

- **SVC instruction** — cooperative syscall.
- **Hardware timer match** — the timeout fired.
- **MemManage / BusFault / UsageFault** — the user program misbehaved.

Each is an exception. The CPU automatically pushes the user frame on PSP
and switches to handler mode on MSP with the kernel privileged again.

## Returning to kernel context

`exit`, fault, and timeout all need to terminate the user program and
resume the kernel's main loop. The mechanism is a hand-rolled
`setjmp/longjmp` implemented inside an exception handler:

1. Before launching, the kernel stashes `{R4..R11, MSP, return_PC}` in
   a static.
2. The relevant exception handler reads the exit status (or fault
   reason), snapshots `DWT.CYCCNT`, then **rewrites the stacked
   exception frame on MSP** so it points back to the kernel's
   `return_PC` with R0 = status.
3. Sets `CONTROL = 0b00` (MSP, privileged), `ISB`.
4. Returns from exception with `EXC_RETURN = 0xFFFFFFF9` (thread mode,
   MSP). The kernel resumes at the saved label, MPU is then disabled
   and SysTick resumes.

This is the trickiest piece in the whole project. The mechanism is
identical regardless of which handler took the abort — SVCall, MemManage,
BusFault, UsageFault, or the timer match.

## Timing strategy

Cycle accuracy is the primary product of the system, so the design
prioritizes it over throughput:

- **Resolution.** SysTick at 1 kHz is too coarse for small algorithms
  (1 ms is millions of cycles at 96 MHz). The DWT cycle counter
  (`DWT.CYCCNT`, 32-bit free-running at CPU clock) is the measurement
  source — ~10 ns resolution at 96 MHz.
- **Perturbation.** SysTick is **paused** for the duration of a user
  run; interrupts during measurement add nondeterministic cycles.
- **Timeout source.** A hardware timer match fires **once**, exactly at
  `start_cycles + (timeout_ms × cycles_per_ms)`. No per-tick polling
  overhead.
- **Clock stability.** The LPC1768 PLL drives CCLK at a fixed 96 MHz
  off the 12 MHz crystal (~50 ppm). Measurements are repeatable
  cross-run.
- **Reporting.** RESULT carries `cycles` *and* `cclk_hz`; the host
  computes wall time as `cycles / cclk_hz`. Decoupling unit from rate
  means changing the clock later doesn't break older tooling.

## Syscall ABI

User code invokes `svc #N`:

| N | Name           | Args            | Return                                              |
|---|----------------|-----------------|-----------------------------------------------------|
| 0 | `exit`         | R0 = code (u32) | does not return                                     |
| 1 | `time_cycles`  | —               | R0:R1 = u64 monotonic CPU cycles since program start |

The SVCall handler reads the immediate from the stacked PC's preceding
halfword, dispatches via a literal `match`, writes returns into the
stacked R0/R1, and `EXC_RETURN`s. `exit` follows the resume-kernel path
above; an unknown SVC immediate is treated as a usage fault.

`time_cycles` is included because it's nearly free (subtract a snapshot,
return). Self-timing isn't the primary measurement channel — the RESULT
frame is — but it lets user programs do intra-program timing. The call
itself takes ~tens of cycles (SVC entry, dispatch, exit); that overhead
is included in the returned delta.

## Fault handling

MemManage, BusFault, and UsageFault are individually vector-enabled (via
SHCSR) so each abort reason stays distinct instead of escalating to
HardFault.

- **MemManage** — reads MMFAR + CFSR, reports the faulting address (or
  CFSR if MMFAR is invalid) as the payload. Aborts the run with
  `status = MEMFAULT`.
- **BusFault** — same shape, reports BFAR. `status = BUSFAULT`.
- **UsageFault** — reports CFSR. `status = USAGEFAULT`.
- **HardFault** — should now only fire for kernel bugs. Panics.

The MPU's W^X enforcement means "user tried to execute its data region"
and "user touched kernel memory" both surface here as MemManage.

## Compile-side: `userlib` + packaging

A user program is a no_std crate that depends on `userlib`:

```rust
#![no_std]
#![no_main]

fn main() {
    userlib::exit(42);
}

userlib::entry!(main);
```

`userlib` provides:

- `exit(code)` and `time_cycles()` — `asm!("svc #N", ...)` shims.
- An `entry!` macro that emits a `pub extern "C" fn _start()` in the
  `.text._start` section.
- A default `#[panic_handler]` (feature-gated) that exits with
  `0xFFFF_FFFF`.
- A `user.x` linker script generated by `userlib/build.rs` for the
  active board feature. The script anchors `.text._start` at offset 0
  of the user_text region, places `.text + .rodata` there, and puts
  `.data + .bss` at user_data.

Consumer crates pull the script in via a build.rs that reads
`DEP_USERLIB_USER_X_DIR` and adds `-Tuser.x`:

```rust
// examples/build.rs
let dir = env::var("DEP_USERLIB_USER_X_DIR").unwrap();
println!("cargo:rustc-link-search={dir}");
println!("cargo:rustc-link-arg-bins=-Tuser.x");
```

The four `examples/` programs (`exit42`, `spin`, `naughty`, `hello`) are
the canonical fixtures.

### Packing an ELF into a `.bin`

`monolink pack` reads the user ELF and emits a header + flat binary:

```sh
./scripts/monolink.sh pack target/thumbv7m-none-eabi/release/exit42 /tmp/exit42.bin
# wrote /tmp/exit42.bin (44 bytes): text=12 data=0 bss=0 entry=0x0 timeout=100ms
```

It uses the `object` crate to read `.text`, `.data`, `.bss` section
sizes and the address of `_start` (masking off the Thumb bit). With
userlib's `entry!` macro pinning `_start` to offset 0, `entry_offset` is
zero in practice — but the tool computes it from the symbol so a future
linker-script change can't silently break entry resolution.

### Uploading

```sh
./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/exit42.bin
# status=OK exit=0x0000002a cycles=4738 cclk_hz=96000000 (49.4 us)
```

For QEMU testing, launch with `-serial pty` and point `monolink upload`
at the PTY path QEMU prints on startup.

## Reference: source layout

| Path                        | What it owns                                                     |
|-----------------------------|------------------------------------------------------------------|
| `kernel/`                   | Kernel firmware. Boots cortex-m-rt, MPU + privilege drop, monolink loader, fault & timer handlers. |
| `kernel/src/user.rs`        | The heart of the runtime: privilege transition, syscall dispatch, fault handling, longjmp. |
| `kernel/src/link.rs`        | Board-coupled glue over `monolink-proto` (read from UART RX ring, send framed bytes). |
| `monolink-proto/`           | Wire format shared by kernel + host: types, CRC32, parser, NakReason. |
| `userlib/`                  | Syscall shims, `entry!` macro, `panic_handler`, `user.x` template. |
| `examples/`                 | Four canonical user programs. Builds for either board.           |
| `tools/monolink/`           | Host CLI. Builds for the host triple, not thumbv7m. `upload` / `pack` / `demo`. |
| `scripts/monolink.sh`       | Wrapper that picks the host triple, since workspace cargo config pins thumbv7m. |
| `scripts/flash-lpc1768.sh`  | probe-rs invocation for the mbed LPC1768.                        |
| `probe-rs-targets/`         | Chip-description YAML for the LPC1768 (not in probe-rs's built-in database). |
