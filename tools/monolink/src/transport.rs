//! [`Transport`] trait: deadline-aware byte read on top of `Write`.
//!
//! The protocol loader needs to read one byte at a time with a phase-specific
//! deadline (e.g. 5 s for ACK, 30 s for CASE_RESULT) without blocking the
//! caller forever on a stalled device. Different host environments do this
//! differently — POSIX `poll(2)` on a `File`, `serialport::SerialPort::set_timeout`,
//! a tokio async stream — so the loader is generic over this trait and
//! callers supply the concrete implementation.

use std::io::{self, Write};
use std::time::Instant;

/// Deadline-aware single-byte read over a writable byte stream.
///
/// Implementors block until either one byte arrives or the supplied
/// [`Instant`] passes. EOF and timeout both surface as `Ok(None)` — the
/// loader treats them the same (no more data, give up on this frame).
pub trait Transport: Write {
    /// Read one byte, returning `Ok(None)` if `deadline` is reached first.
    fn read_byte_with_deadline(&mut self, deadline: Instant) -> io::Result<Option<u8>>;

    /// Discard any bytes buffered in the underlying device's input queue.
    ///
    /// Called by [`Loader::upload_and_run`](crate::Loader::upload_and_run)
    /// after the initial settle pause so stale boot-log bytes from the
    /// device don't get misframed as the first SOF. Default: no-op.
    fn clear_input(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---- POSIX File-based transport -------------------------------------------

#[cfg(feature = "posix-transport")]
pub use posix::{FileTransport, open_serial};

#[cfg(feature = "posix-transport")]
mod posix {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsFd, BorrowedFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::Instant;

    use anyhow::{Context, Result};
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use nix::sys::termios::{self, BaudRate, SetArg};

    use super::Transport;

    /// `File`-backed transport using `poll(2)` for deadline reads.
    ///
    /// Works for both real USB-CDC devices and QEMU `-serial pty` PTYs:
    /// termios calls that ENOTTY on a PTY are silently no-op'd, matching
    /// the prior Python tool's behavior.
    pub struct FileTransport(pub File);

    impl FileTransport {
        pub fn new(file: File) -> Self {
            Self(file)
        }

        pub fn into_inner(self) -> File {
            self.0
        }
    }

    impl Write for FileTransport {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    impl Transport for FileTransport {
        fn read_byte_with_deadline(&mut self, deadline: Instant) -> io::Result<Option<u8>> {
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let remaining_ms = u16::try_from((deadline - now).as_millis().min(u16::MAX as u128))
                .expect("clamped above");
            let fd: BorrowedFd<'_> = self.0.as_fd();
            let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
            match poll(&mut fds, PollTimeout::from(remaining_ms)) {
                Ok(0) => Ok(None),
                Ok(_) => {
                    let mut buf = [0u8; 1];
                    match self.0.read(&mut buf) {
                        Ok(0) => Ok(None),
                        Ok(_) => Ok(Some(buf[0])),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(io::Error::other(format!("poll: {e}"))),
            }
        }

        fn clear_input(&mut self) -> io::Result<()> {
            match termios::tcflush(self.0.as_fd(), termios::FlushArg::TCIOFLUSH) {
                Ok(()) | Err(nix::errno::Errno::ENOTTY) => Ok(()),
                Err(e) => Err(io::Error::other(format!("tcflush: {e}"))),
            }
        }
    }

    /// Open a serial port (or PTY) and configure raw 8N1 at `baud`.
    ///
    /// On PTYs (`qemu -serial pty`) `tcgetattr`/`tcsetattr` return ENOTTY
    /// and we silently skip — PTYs don't have a real baud rate, the
    /// framing just rides on top.
    pub fn open_serial(path: &str, baud: u32) -> Result<FileTransport> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)
            .with_context(|| format!("opening {path}"))?;

        if let Ok(mut t) = termios::tcgetattr(file.as_fd()) {
            termios::cfmakeraw(&mut t);
            if let Some(b) = map_baud(baud) {
                let _ = termios::cfsetispeed(&mut t, b);
                let _ = termios::cfsetospeed(&mut t, b);
            }
            let _ = termios::tcsetattr(file.as_fd(), SetArg::TCSANOW, &t);
            let _ = termios::tcflush(file.as_fd(), termios::FlushArg::TCIOFLUSH);
        }

        Ok(FileTransport(file))
    }

    fn map_baud(rate: u32) -> Option<BaudRate> {
        Some(match rate {
            9600 => BaudRate::B9600,
            19200 => BaudRate::B19200,
            38400 => BaudRate::B38400,
            57600 => BaudRate::B57600,
            115200 => BaudRate::B115200,
            230400 => BaudRate::B230400,
            _ => return None,
        })
    }
}
