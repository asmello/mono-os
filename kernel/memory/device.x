/* Weak-alias overrides for device-specific interrupts.
 *
 * cortex-m-rt's link.x INCLUDEs this file when the `device` feature is
 * enabled. We provide our `__INTERRUPTS` directly in Rust (per board), so
 * this file only needs to exist — every name referenced from `__INTERRUPTS`
 * is already defined in Rust source. */
