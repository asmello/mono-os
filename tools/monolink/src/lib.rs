//! Host-side library for the monolink wire protocol.
//!
//! Wraps [`monolink_proto`] (the framing/types shared with the kernel) with
//! a streaming [`Loader`] that drives the upload → case-run → result flow
//! over any [`Transport`], plus [`pack_into`] / [`pack_paths`] for turning a
//! userlib-linked ELF into a monoexec `.bin`.
//!
//! The CLI in `src/bin/monolink.rs` is a thin wrapper; out-of-process
//! callers (the `exact-runner` Pi agent, the `exact-api` build worker)
//! consume this crate directly with `default-features = false` and provide
//! their own transport (e.g. `serialport`).

pub use monolink_proto as proto;

mod frame;
mod loader;
mod pack;
mod transport;

pub mod demo;

pub use loader::{Loader, RunReport};
pub use pack::{PackSummary, pack_into, pack_paths};
pub use transport::Transport;

#[cfg(feature = "posix-transport")]
pub use transport::{FileTransport, open_serial};
