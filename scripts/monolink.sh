#!/usr/bin/env bash
# Wrapper around `cargo run` for tools/monolink. The host tool needs to build
# for the host triple, but the workspace-level .cargo/config.toml pins the
# default target to thumbv7m-none-eabi for the firmware crates. We override
# per-invocation with --target $(rustc host).
set -euo pipefail

cd "$(dirname "$0")/.."

HOST=$(rustc -vV | sed -n 's/host: //p')

exec cargo run \
    --manifest-path tools/monolink/Cargo.toml \
    --release \
    --target "$HOST" \
    -- "$@"
