#!/usr/bin/env bash
# Invoked by cargo as the runner for kernel test binaries built for
# x86_64-unknown-none. Detects the host triple so the nested `cargo run`
# for the runner crate doesn't inherit the bare-metal target.
set -euo pipefail

host=$(rustc -vV | sed -n 's/^host: //p')
if [[ -z "$host" ]]; then
  echo "run-kernel-test.sh: failed to detect host triple from rustc -vV" >&2
  exit 1
fi

exec cargo run --quiet --release --package mono-os --target "$host" -- "$@"
