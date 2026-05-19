# mono-os

A small Cortex-M3 runtime that loads one precompiled program at a time, runs it under hardware memory protection with a fixed deadline, and reports cycle-accurate timing back to the host. Targets QEMU's `lm3s6965evb` machine and the mbed NXP LPC1768.

The repo started as a port of Phil Opp's [Writing an OS in Rust](https://os.phil-opp.com/) to Cortex-M; the deterministic-single-program runtime is what it's grown into. The original port plan and rationale live in [`docs/arm-port.md`](docs/arm-port.md).

## Status

- Boot via `cortex-m-rt` (vector table + reset handler).
- Per-board UART drivers (Stellaris for QEMU, LPC 16550-style for the LPC1768), interrupt-driven RX into an SPSC ring.
- SysTick at 1 kHz, DWT cycle counter exposed as a u64.
- MPU + unprivileged-thread-mode privilege drop with a small in-RAM trampoline; SVC + MemManage/BusFault/UsageFault routed back through a hand-rolled longjmp.
- Hardware timer match interrupt as the per-run deadline source; SysTick paused while a user program runs.
- `monolink` v0 loader: stop-and-wait framing over UART with structured NAK reasons, shared between the kernel and the host tool via the `monolink-proto` crate.
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

The `monolink` host tool ships a `.bin` to a running kernel and prints the result. The wrapper script picks the host triple so cargo doesn't try to build it for thumbv7m.

```sh
./scripts/monolink.sh demo /tmp/exit42.bin                       # synth a tiny exit(42) .bin
./scripts/monolink.sh upload /dev/cu.usbmodem* /tmp/exit42.bin   # ship + run + read RESULT
```

For QEMU testing point it at the PTY printed by `qemu -serial pty` instead of the USB-CDC device.

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

tools/monolink/            # host loader (excluded from the firmware workspace,
└── src/main.rs            # builds for the host triple). nix-based termios + poll.

probe-rs-targets/
└── LPC1700_Series.yaml    # chip target generated from Keil.LPC1700_DFP,
                           # hand-patched to mark IRAM1 executable

scripts/
├── flash-lpc1768.sh       # build + probe-rs download + probe-rs reset
└── monolink.sh            # cargo-run wrapper for the host tool
```

## License

MIT — see [`LICENSE`](LICENSE).
