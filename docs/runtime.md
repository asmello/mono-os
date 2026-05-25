# mono-os runtime architecture

This document describes the current runtime: what's on the device,
what the host ships to it, and the contract between the two. The
original Phil-Opp port plan the project grew out of is preserved at
[`docs/history/arm-port.md`](history/arm-port.md).

## What this is

mono-os runs **one precompiled user program** under hardware memory
protection. The host drives that program against one or more *cases* —
each case is its own program invocation with its own input bytes and
its own per-case deadline; the kernel reports cycle-accurate timing
and any output bytes back per case. The motivation is deterministic
benchmarking: removing context switching, demand paging, and dynamic
scheduling lets small algorithms be measured with cycle-stable timing
across many inputs.

The kernel is bare-metal Rust on ARMv7-M (Cortex-M3). User programs are
flat binaries linked against a small `userlib` and shipped over UART.
There is no scheduler, no heap, no filesystem, no second user program.
Case state is reset between invocations (`.data` re-initialized, `.bss`
zeroed) so two runs of the same program against the same input
produce the same cycle count.

Supported boards:

| Board           | Use                                                       |
|-----------------|-----------------------------------------------------------|
| `lm3s6965evb`   | QEMU emulation — the fast dev loop.                       |
| LPC1768 (mbed)  | Real hardware. 96 MHz, 64 KiB SRAM (32 K local + 2×16 K AHB). |

## Memory layout

The kernel claims one 32 KiB SRAM bank for itself (MSP, `.data`, `.bss`);
user code gets a separate 32 KiB window carved into three power-of-2 MPU
regions. On LPC1768 those regions live in the two AHB SRAM banks at
`0x2007_C000` and `0x2008_0000`, leaving the full 32 KiB local SRAM
(`0x1000_0000`) for the kernel.

```
0x2007_C000 ┬─────────────────────────┐
            │ user .text (R+X)         │  8 KiB  MPU region 0 (unpriv RX)
0x2007_E000 ┼─────────────────────────┤
            │ user .data / .bss        │  8 KiB  MPU region 1 (unpriv RW, XN)
0x2008_0000 ┼─────────────────────────┤
            │ user stack (PSP)         │ 16 KiB  MPU region 2 (unpriv RW, XN)
0x2008_4000 ┴─────────────────────────┘
```

(LPC1768 addresses. On `lm3s6965evb` the user window lives at
`0x2000_8000`..`0x2001_0000` in the single SRAM bank; same 8 / 8 / 16 KiB
split, kernel keeps `0x2000_0000`..`0x2000_8000`. Both boards' user-side
offsets are identical relative to `USER_TEXT_BASE`.)

Because user code on LPC1768 executes from AHB SRAM rather than the
CPU-local data bus, absolute cycle counts differ from any pre-`6de4a31`
measurements. Timings are still bit-exact run-to-run on the same
hardware; they're just not comparable to pre-bump baselines.

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

Sizes must satisfy `text_size ≤ 8 KiB - 12` and
`data_size + bss_size ≤ 8 KiB`. The kernel rejects anything else with a
NAK before execution.

The header is an **internal contract** between the kernel and the
compile-side `userlib`. It changes freely in lockstep — there is no
backwards-compatibility surface for external `.bin`s. `version` exists
only as a sanity gate against accidental mismatches.

## monolink wire protocol

Stop-and-wait framing over UART, host-initiated. One frame at a time,
no flow control beyond ACK/NAK. The protocol has two phases — an
*upload phase* that loads a program, then a *case phase* that drives
it against zero or more cases:

```
# Upload phase
host → device:   START_LOAD                →  device ACKs (or NAKs)
host → device:   N CHUNKs (program body)   →  device ACKs each
host → device:   EOF                       →  device ACKs, snapshots
                                              .data, enters case loop

# Case phase (repeat per case)
host → device:   M CASEs (input bytes)     →  device ACKs each
host → device:   CASE_END                  →  device relaunches program
                                              with fresh state
                                          ←  Q CASE_OUTPUTs (user write())
                                          ←  CASE_RESULT{status, exit, cycles}

# End of run
host → device:   RUN_END                   →  device exits case loop
                                          ←  RESULT{overall status}
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

| TYPE | Direction  | Name          | Payload                          |
|------|------------|---------------|----------------------------------|
| 0x01 | host → dev | START_LOAD    | 32-byte `ExecHeader`             |
| 0x02 | host → dev | CHUNK         | up to 256 bytes of program body  |
| 0x03 | host → dev | EOF           | empty                            |
| 0x04 | host → dev | CASE          | up to 256 bytes of case input    |
| 0x05 | host → dev | CASE_END      | empty (triggers launch)          |
| 0x06 | host → dev | RUN_END       | empty (closes the run)           |
| 0x10 | dev → host | ACK           | empty                            |
| 0x11 | dev → host | NAK           | 1-byte `NakReason`               |
| 0x20 | dev → host | RESULT        | 20-byte overall summary          |
| 0x21 | dev → host | CASE_OUTPUT   | up to 256 bytes of user output   |
| 0x22 | dev → host | CASE_RESULT   | 16-byte per-case summary         |

The chunked split between `CASE` and `CASE_END` (and symmetrically
`CASE_OUTPUT` and `CASE_RESULT`) means single frames stay ≤ 256
payload bytes — short enough for the CRC to be useful against line
noise — while supporting cases that are arbitrarily larger than one
frame. Both input and output are capped at 1 KiB total per case (the
device-side buffer); larger needs a config bump.

`CASE_END` and `CASE_OUTPUT` / `CASE_RESULT` are unacknowledged: the
`CASE_RESULT` *is* the acknowledgment that the case completed. CRC-
corrupted frames still surface as NAKs, and the host retries the
offending frame before sending the next.

NAK reasons (`monolink-proto::NakReason`) cover framing errors
(oversized frame, CRC mismatch, unknown type), session-sequencing
errors (chunk before start_load, case before run, case-input
overflow), and loader errors (bad header magic, oversized text or
data). The kernel never emits raw text mid-session — that would
interleave with framed bytes — so the host tool renders the reason
byte into the user-facing message.

Result payloads:

```rust
#[repr(C)]
struct CaseResultFrame {
    status:    u8,   // 0=OK, 1=TIMEOUT, 2=MEMFAULT, 3=BUSFAULT,
                     // 4=USAGEFAULT, 5=LOAD_ERROR
    _pad:      [u8; 3],
    exit_code: u32,  // arg to exit() on OK; fault payload otherwise
    cycles:    u64,  // CPU cycles between case launch and termination
}

#[repr(C)]
struct ResultFrame {  // overall, one per upload
    status:    u8,
    _pad:      [u8; 3],
    exit_code: u32,  // unused for case-driven runs (per-case data is
                     // in the CASE_RESULTs)
    cycles:    u64,  // unused for case-driven runs
    cclk_hz:   u32,  // CCLK the per-case `cycles` counts were taken at
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

## Case lifecycle

Programs run **one case at a time**. From the kernel's point of view, a
case is one full invocation of the user program: relaunch from
`_start`, run until `exit` / fault / timeout, report. State doesn't
carry across cases.

Between cases the kernel:

1. **Restores `.data`.** A copy of the initialized data image was
   snapshotted out of `USER_DATA_BASE` at EOF (right after the loader
   finished writing the body). On each case the kernel `memcpy`s that
   image back, then `memset`s the `.bss` tail. The user always sees
   the same initial state.
2. **Resets the case I/O buffers.** A 1 KiB kernel-side input buffer
   was filled by the most recent `TYPE_CASE` chunks; the read cursor
   is rewound to 0. A 1 KiB output buffer is cleared.
3. **Relaunches.** Same trampoline copy / MPU enable / timeout arm /
   DWT snapshot / `launch_user_asm` as the very first launch — there
   is no "fast path" for second-and-subsequent cases.

The per-case relaunch overhead is consistent across cases (a few µs at
96 MHz on LPC1768 — bounded by the `.data` memcpy size) and isn't
included in the per-case cycle count, since `DWT.CYCCNT` is snapshotted
*after* the reset. For most benchmarks the case-to-case overhead is
dwarfed by the user program's own runtime.

A case fault or timeout aborts the whole run. The kernel still sends
the `CASE_RESULT` for the failing case (with the fault status and a
truncated `cycles`), then the final `RESULT` with the same status, and
returns to the wait-for-`START_LOAD` outer loop. Successful preceding
cases keep their results.

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

| N | Name          | Args                                | Return                                       |
|---|---------------|-------------------------------------|----------------------------------------------|
| 0 | `exit`        | R0 = code (u32)                     | does not return; ends the case               |
| 1 | `time_cycles` | —                                   | R0:R1 = u64 cycles since case launch         |
| 2 | `read`        | R0 = buf ptr, R1 = max bytes        | R0 = bytes copied (0 once input exhausted)   |
| 3 | `write`       | R0 = buf ptr, R1 = byte count       | —                                            |

The SVCall handler reads the immediate from the stacked PC's preceding
halfword, dispatches via a literal `match`, writes returns into the
stacked R0/R1, and `EXC_RETURN`s. `exit` follows the resume-kernel
path above and ends the case; an unknown SVC immediate is treated as
a usage fault.

`read` drains the per-case input buffer the host streamed in via
`TYPE_CASE` frames. Calls return successive prefixes; once the buffer
is empty, `read` returns 0. `write` appends to a kernel-side output
buffer (1 KiB cap); the bytes are sent back to the host as
`TYPE_CASE_OUTPUT` frames *after* `exit()`, so write overhead doesn't
inflate the measured cycle count for the case. Writing past the cap
aborts the case with a usage fault.

`time_cycles` is included because it's nearly free (subtract a
snapshot, return). Self-timing isn't the primary measurement channel —
the `CASE_RESULT` frame is — but it lets user programs do intra-case
timing. The call itself takes ~tens of cycles (SVC entry, dispatch,
exit); that overhead is included in the returned delta.

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

A user program is a no_std crate that depends on `userlib`. The
single-shot shape (one program invocation, no input) is just `exit`:

```rust
#![no_std]
#![no_main]

fn main() {
    userlib::exit(42);
}

userlib::entry!(main);
```

The case-driven shape adds `read` for input and `write` for output.
Notice there's no loop — `exit` ends *the case*, not the run, so the
kernel relaunches `main` for each input the host streams in:

```rust
#![no_std]
#![no_main]

fn main() {
    let mut buf = [0u8; 4];
    let n = userlib::read(&mut buf);
    let v = u32::from_le_bytes(buf[..n].try_into().unwrap());
    let sum: u64 = (1..=v as u64).sum();
    userlib::write(&sum.to_le_bytes());
    // entry!'s _start calls exit(0) when main returns, ending the case.
}

userlib::entry!(main);
```

`userlib` provides:

- `exit(code)`, `time_cycles()`, `read(&mut [u8]) -> usize`,
  `write(&[u8])` — `asm!("svc #N", ...)` shims.
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

The `examples/` programs are the canonical fixtures: `exit42`, `spin`,
`naughty`, `hello` (single-shot), and `sum_to_n` (case-driven).

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

Single-shot programs run a single 1-case run with empty input — the
host implicitly sends one `CASE_END` after `EOF`:

```sh
./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/exit42.bin
# case[0] status=OK exit=0x0000002a cycles=4863 output=(none)
# RESULT status=OK cclk_hz=96000000
```

Case-driven programs take a `--cases <file>` argument. The cases file
is a sequence of `[u16 LE length][bytes]` records — each record is
one case's input bytes. For `sum_to_n` running `n ∈ {1, 10, 100,
1000}`:

```sh
printf '\x04\x00\x01\x00\x00\x00\x04\x00\x0a\x00\x00\x00\x04\x00\x64\x00\x00\x00\x04\x00\xe8\x03\x00\x00' > /tmp/sums.cases
./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/sum_to_n.bin --cases /tmp/sums.cases
# case[0] status=OK exit=0x00000000 cycles=7880 output=0x0100000000000000     (1)
# case[1] status=OK exit=0x00000000 cycles=7905 output=0x3700000000000000     (55)
# case[2] status=OK exit=0x00000000 cycles=7905 output=0xba13000000000000     (5050)
# case[3] status=OK exit=0x00000000 cycles=7905 output=0x14a3070000000000     (500500)
# RESULT status=OK cclk_hz=96000000
```

The output bytes are whatever the user wrote — the host doesn't
interpret them; that's the wrapper layer's job.

For QEMU testing, launch with `-serial pty` and point `monolink
upload` at the PTY path QEMU prints on startup.

## Reference: source layout

| Path                        | What it owns                                                     |
|-----------------------------|------------------------------------------------------------------|
| `kernel/`                   | Kernel firmware. Boots cortex-m-rt, MPU + privilege drop, monolink loader + case loop, fault & timer handlers. |
| `kernel/src/main.rs`        | Outer state machine: upload phase → case loop → final RESULT.    |
| `kernel/src/user.rs`        | Privilege transition, syscall dispatch (`exit` / `time_cycles` / `read` / `write`), fault handling, longjmp, `CASE_IO` and `DATA_INIT_IMAGE` statics. |
| `kernel/src/link.rs`        | Board-coupled glue over `monolink-proto` (read from UART RX ring, send framed bytes, `send_case_result`). |
| `monolink-proto/`           | Wire format shared by kernel + host: types, CRC32, parser, NakReason. |
| `userlib/`                  | Syscall shims (`exit` / `time_cycles` / `read` / `write`), `entry!` macro, `panic_handler`, `user.x` template. |
| `examples/`                 | Canonical user programs (`exit42`, `spin`, `naughty`, `hello`, `sum_to_n`). Builds for either board. |
| `tools/monolink/`           | Host CLI. Builds for the host triple, not thumbv7m. `upload` / `pack` / `demo`. |
| `scripts/monolink.sh`       | Wrapper that picks the host triple, since workspace cargo config pins thumbv7m. |
| `scripts/flash-lpc1768.sh`  | probe-rs invocation for the mbed LPC1768.                        |
| `probe-rs-targets/`         | Chip-description YAML for the LPC1768 (not in probe-rs's built-in database). |
