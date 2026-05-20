# mono-os

A small Cortex-M3 runtime that loads one precompiled program at a time, runs it under hardware memory protection with a fixed deadline, and reports cycle-accurate timing back to the host. Targets QEMU's `lm3s6965evb` machine and the mbed NXP LPC1768.

The repo started as a port of Phil Opp's [Writing an OS in Rust](https://os.phil-opp.com/) to Cortex-M; the deterministic-single-program runtime is what it's grown into. Architecture is documented in [`docs/runtime.md`](docs/runtime.md); the original port plan is preserved at [`docs/history/arm-port.md`](docs/history/arm-port.md).

## Status

- Boot via `cortex-m-rt` (vector table + reset handler).
- Per-board UART drivers (Stellaris for QEMU, LPC 16550-style for the LPC1768), interrupt-driven RX into an SPSC ring.
- SysTick at 1 kHz, DWT cycle counter exposed as a u64.
- MPU + unprivileged-thread-mode privilege drop with a small in-RAM trampoline; SVC + MemManage/BusFault/UsageFault routed back through a hand-rolled longjmp.
- Hardware timer match interrupt as the per-case deadline source; SysTick paused while a user program runs.
- `monolink` v0 loader: stop-and-wait framing over UART with structured NAK reasons, shared between the kernel and the host tool via the `monolink-proto` crate.
- Case-driven I/O: user programs read input via `read(&mut [u8])` and produce output via `write(&[u8])`; the host streams one or more cases per upload and gets back per-case cycle counts + outputs. State is reset between cases (`.data` re-initialized, `.bss` zeroed) so each case is reproducible.
- Custom no-std test harness; results stream over UART, pass/fail signalled to cargo via QEMU semihosting exit codes.

## Dev loop

`nix develop` provides nightly Rust with `thumbv7m-none-eabi`, `qemu`, `probe-rs-tools`, `cargo-binutils`, and `tio`.

### QEMU

```sh
cargo run                       # boots qemu-system-arm -machine lm3s6965evb, prints over UART
cargo test                      # custom harness in QEMU; semihosting exit codes
```

### mbed LPC1768

```sh
./scripts/flash-lpc1768.sh      # builds for lpc1768, flashes via probe-rs, resets
tio /dev/cu.usbmodem* -b 115200 # open the mbed's USB CDC port
```

probe-rs drives the mbed's onboard CMSIS-DAP probe over SWD. The LPC1768 isn't in probe-rs's built-in chip database, so we supply one generated from NXP's CMSIS-Pack at [`probe-rs-targets/LPC1700_Series.yaml`](probe-rs-targets/LPC1700_Series.yaml) (the script wires the path via `PROBE_RS_CHIP_DESCRIPTION_PATH`). UART output comes back through a separate USB CDC ACM device the mbed interface MCU presents.

### Uploading a user program

A user program is a no_std crate that depends on `userlib` (syscall shims for `exit` / `time_cycles` / `read` / `write`, an `entry!` macro, default panic handler, and a board-specific `user.x` linker script). The programs in `examples/` are the canonical fixtures.

Single-shot (no input):

```sh
# Build for the host's board (default is lm3s6965evb; pass --no-default-features --features lpc1768 for hardware).
cargo build --release -p examples --bin exit42

# Pack the ELF into a monoexec .bin (32-byte header + flat body).
./scripts/monolink.sh pack target/thumbv7m-none-eabi/release/exit42 /tmp/exit42.bin

# Ship to a running kernel. With no --cases the host runs exactly one
# 1-case run with empty input — fine for programs that just call exit().
./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/exit42.bin
# case[0] status=OK exit=0x0000002a cycles=4863 output=(none)
# RESULT status=OK cclk_hz=96000000
```

Case-driven (one program, many inputs):

```sh
cargo build --release -p examples --bin sum_to_n
./scripts/monolink.sh pack target/thumbv7m-none-eabi/release/sum_to_n /tmp/sum_to_n.bin

# Cases file: a sequence of [u16 LE length][bytes] records.
# Four 4-byte u32 LE inputs (n = 1, 10, 100, 1000):
printf '\x04\x00\x01\x00\x00\x00\x04\x00\x0a\x00\x00\x00\x04\x00\x64\x00\x00\x00\x04\x00\xe8\x03\x00\x00' > /tmp/sums.cases

./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/sum_to_n.bin --cases /tmp/sums.cases
# case[0] status=OK exit=0x0 cycles=7880 output=0x0100000000000000     (sum=1)
# case[1] status=OK exit=0x0 cycles=7905 output=0x3700000000000000     (sum=55)
# case[2] status=OK exit=0x0 cycles=7905 output=0xba13000000000000     (sum=5050)
# case[3] status=OK exit=0x0 cycles=7905 output=0x14a3070000000000     (sum=500500)
# RESULT status=OK cclk_hz=96000000
```

For QEMU testing point `upload` at the PTY printed by `qemu -serial pty` instead of the USB-CDC device.

The `examples/` set covers the spectrum: `exit42` (exit code), `spin` (timeout), `naughty` (MemManage), `hello` (`time_cycles` round-trip), and `sum_to_n` (case-driven). See [`docs/runtime.md`](docs/runtime.md) for the syscall ABI, executable format, wire protocol, and per-case relaunch lifecycle.

## Layout

```
kernel/
├── Cargo.toml             # features: lm3s6965evb (default) | lpc1768
├── build.rs               # copies memory/<board>.x to OUT_DIR
├── memory/
│   ├── lm3s6965evb.x
│   └── lpc1768.x
├── src/
│   ├── lib.rs             # kernel::init(), wfi_loop()
│   ├── main.rs            # #[entry], monolink dispatch loop, panic handler
│   ├── clock.rs           # DWT bring-up, software-extended u64 cycle counter
│   ├── time.rs            # SysTick, tick counter, exception handler
│   ├── timeout.rs         # per-run deadline timer (board-routed)
│   ├── mpu.rs             # MPU region encoding + enable/disable
│   ├── user.rs            # privilege drop, syscall + fault handling, loader
│   ├── link.rs            # board-coupled glue over monolink-proto
│   ├── ring.rs            # SPSC byte ring for UART RX
│   ├── print.rs           # println! macro -> board::_print
│   ├── testing.rs         # custom test harness + test_panic_handler
│   └── board/
│       ├── mod.rs         # Board + Console + UartRx/Tx traits, LineMode CRLF adapter
│       ├── lm3s6965evb.rs
│       └── lpc1768.rs
└── tests/                 # one binary per integration test
    ├── basic_boot.rs
    ├── systick.rs
    ├── cycles.rs
    ├── timeout.rs
    ├── m1_user.rs
    └── should_panic.rs

monolink-proto/            # no_std crate shared by kernel + host: frame types,
└── src/lib.rs             # ExecHeader, ResultFrame, CRC32, parser, NakReason

userlib/                   # compile-side counterpart: syscall shims, entry!
├── src/lib.rs             # macro, default panic_handler.
├── user.x.in              # linker-script template; build.rs substitutes
└── build.rs               # USER_CODE_BASE / USER_DATA_BASE per board.

examples/                  # canonical user programs: exit42, spin (timeout),
└── src/bin/               # naughty (memfault), hello (time_cycles),
                           # sum_to_n (case-driven read/write).

tools/monolink/            # host loader (excluded from the firmware workspace,
└── src/main.rs            # builds for the host triple). nix-based termios +
                           # poll; `pack` reads ELFs via the `object` crate.

probe-rs-targets/
└── LPC1700_Series.yaml    # chip target generated from Keil.LPC1700_DFP,
                           # hand-patched to mark IRAM1 executable

scripts/
├── flash-lpc1768.sh       # build + probe-rs download + probe-rs reset
└── monolink.sh            # cargo-run wrapper for the host tool
```

## License

MIT — see [`LICENSE`](LICENSE).
