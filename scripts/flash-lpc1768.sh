#!/usr/bin/env bash
# Build the kernel for the mbed NXP LPC1768 and flash it via probe-rs over the
# onboard CMSIS-DAP probe. The LPC1768 chip definition isn't shipped with
# probe-rs, so we supply one generated from NXP's CMSIS-Pack (see
# probe-rs-targets/LPC1700_Series.yaml). Open `/dev/cu.usbmodem*` in a separate
# terminal at 9600 8N1 to see UART output.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --no-default-features --features lpc1768

export PROBE_RS_CHIP_DESCRIPTION_PATH=probe-rs-targets/LPC1700_Series.yaml
export PROBE_RS_CHIP=LPC1768
export PROBE_RS_SPEED=1000

# Both calls below complete the operation successfully (download writes flash;
# reset releases the core), but `session_drop` then fails to clear the FP_COMP
# hardware breakpoint registers: the LPC17xx leaves SWD briefly unreliable
# after IAP / reset, and probe-rs surfaces that as a non-zero exit. We swallow
# the cleanup error in both cases.
probe-rs download target/thumbv7m-none-eabi/debug/kernel || true
probe-rs reset || true
