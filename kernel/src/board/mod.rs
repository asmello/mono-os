#[cfg(feature = "lm3s6965evb")]
mod lm3s6965evb;
#[cfg(feature = "lm3s6965evb")]
pub use lm3s6965evb::{Uart, _print, init};

#[cfg(feature = "lpc1768")]
mod lpc1768;
#[cfg(feature = "lpc1768")]
pub use lpc1768::{Uart, _print, init};
