#!/usr/bin/env bash
# Build the kernel for the mbed NXP LPC1768 and flash it via OpenOCD over the
# onboard CMSIS-DAP probe. Open `/dev/cu.usbmodem*` in a separate terminal at
# 9600 8N1 to see UART output.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --no-default-features --features lpc1768

openocd \
  -f interface/cmsis-dap.cfg \
  -f target/lpc17xx.cfg \
  -c "program target/thumbv7m-none-eabi/debug/kernel verify reset exit"
