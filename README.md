# mono-os

A Rust bare-metal kernel for ARM Cortex-M3. Runs on QEMU's `lm3s6965evb` machine and on the mbed NXP LPC1768 over the same UART path.

Started from Phil Opp's [Writing an OS in Rust](https://os.phil-opp.com/) at chapter 9 (heap + async setup, x86_64). Retargeted to Cortex-M because the longer-term goal — a single deterministic user program with hand-segmented memory — fits the Cortex-M model better than x86. The retargeting plan and rationale live in [`docs/arm-port.md`](docs/arm-port.md).

## Status

- Boot via `cortex-m-rt` (vector table + reset handler).
- Stellaris UART driver for QEMU, LPC 16550-style UART driver for the LPC1768.
- SysTick at 1 kHz with a monotonic tick counter.
- UART-routed panic handler with `file:line:col` from `PanicInfo`.
- Custom no-std test harness; results stream over UART, pass/fail signalled to cargo via QEMU semihosting exit codes.
- Board abstraction (`Board` + `Console` traits) — adding a third target = one source file + a `memory/<board>.x`.

Next: heap (feature-gated `linked_list_allocator`), HardFault test, board-clock improvements. Eventually smoltcp on the LPC1768/STM32 Ethernet MAC; see [`docs/arm-port.md`](docs/arm-port.md).

## Dev loop

The repo expects `nix develop` for the toolchain. The flake provides nightly Rust with `thumbv7m-none-eabi`, `qemu`, `probe-rs-tools`, and `tio`.

### QEMU

```sh
cargo run     # boots qemu-system-arm -machine lm3s6965evb, prints over UART
cargo test    # runs the custom harness in QEMU; semihosting exit codes
```

### mbed LPC1768

```sh
./scripts/flash-lpc1768.sh      # builds for lpc1768, flashes via probe-rs, resets
tio /dev/cu.usbmodem* -b 9600   # open the mbed's USB CDC port at 9600 8N1
```

probe-rs drives the mbed's onboard CMSIS-DAP probe over SWD. The LPC1768 isn't in probe-rs's built-in chip database, so we supply one generated from NXP's CMSIS-Pack at [`probe-rs-targets/LPC1700_Series.yaml`](probe-rs-targets/LPC1700_Series.yaml) (the script wires the path via `PROBE_RS_CHIP_DESCRIPTION_PATH`). UART output comes back through a separate USB CDC ACM device the mbed interface MCU presents.

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
│   ├── main.rs            # #[entry], UART-routed panic handler, prod loop
│   ├── time.rs            # SysTick, tick counter, exception handler
│   ├── print.rs           # println! macro -> board::_print
│   ├── testing.rs         # custom test harness + test_panic_handler
│   └── board/
│       ├── mod.rs         # Board + Console traits, LineMode CRLF adapter
│       ├── lm3s6965evb.rs
│       └── lpc1768.rs
└── tests/                 # one binary per integration test
    ├── basic_boot.rs
    ├── systick.rs
    ├── should_panic.rs
    └── ...

probe-rs-targets/
└── LPC1700_Series.yaml    # chip target generated from Keil.LPC1700_DFP,
                          # hand-patched to mark IRAM1 executable

scripts/
└── flash-lpc1768.sh       # build + probe-rs download + probe-rs reset
```

## License

MIT — see [`LICENSE`](LICENSE).
