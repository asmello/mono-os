//! monolink: ship a user-program `.bin` to the device, wait for the RESULT.
//!
//! Wire framing and types come from `monolink-proto`, which is the same
//! source the kernel uses — there's no drift-prone parallel definition here.
//!
//! POSIX-only. Built directly on `nix` (termios + poll) so PTYs from
//! `qemu -serial pty` work the same as real USB-CDC devices: termios calls
//! that ENOTTY on a PTY are silently no-op'd, matching the prior Python
//! tool's behavior.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser as ClapParser, Subcommand};
use monolink_proto::{
    BadReason, CHUNK_MAX, Crc32, ExecHeader, NakReason, Parser, Poll as ProtoPoll, ResultFrame,
    SOF, STATUS_BUSFAULT, STATUS_LOAD_ERROR, STATUS_MEMFAULT, STATUS_OK, STATUS_TIMEOUT,
    STATUS_USAGEFAULT, TYPE_ACK, TYPE_CHUNK, TYPE_EOF, TYPE_NAK, TYPE_RESULT, TYPE_START_LOAD,
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::termios::{self, BaudRate, SetArg};

#[derive(ClapParser)]
#[command(about = "Host loader for the monolink wire protocol")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Send a .bin to the device, print the RESULT frame.
    Upload {
        /// Serial device, e.g. /dev/cu.usbmodem* or a QEMU `-serial pty`
        port: String,
        /// Path to a monoexec .bin (header + body)
        bin: PathBuf,
        #[arg(long, default_value_t = 115_200)]
        baud: u32,
    },
    /// Write a tiny exit(42) .bin for sanity tests.
    Demo {
        /// Output path
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Upload { port, bin, baud } => upload(&port, &bin, baud),
        Cmd::Demo { out } => write_demo_bin(&out),
    }
}

// ---- serial open / configure ----------------------------------------------

fn open_serial(path: &str, baud: u32) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(path)
        .with_context(|| format!("opening {path}"))?;

    // Configure raw 8N1 termios at the requested baud. On PTYs (QEMU
    // `-serial pty`) tcgetattr/tcsetattr return ENOTTY and we silently
    // skip — PTYs don't have a real baud rate, the framing just rides
    // on top.
    if let Ok(mut t) = termios::tcgetattr(file.as_fd()) {
        termios::cfmakeraw(&mut t);
        if let Some(b) = map_baud(baud) {
            let _ = termios::cfsetispeed(&mut t, b);
            let _ = termios::cfsetospeed(&mut t, b);
        }
        let _ = termios::tcsetattr(file.as_fd(), SetArg::TCSANOW, &t);
        let _ = termios::tcflush(file.as_fd(), termios::FlushArg::TCIOFLUSH);
    }

    Ok(file)
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

// ---- timed I/O ------------------------------------------------------------

fn read_byte_until(file: &mut File, deadline: Instant) -> Result<Option<u8>> {
    let now = Instant::now();
    if now >= deadline {
        return Ok(None);
    }
    let remaining_ms =
        u16::try_from((deadline - now).as_millis().min(u16::MAX as u128)).expect("clamped above");
    let fd: BorrowedFd<'_> = file.as_fd();
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
    match poll(&mut fds, PollTimeout::from(remaining_ms)) {
        Ok(0) => Ok(None),
        Ok(_) => {
            let mut buf = [0u8; 1];
            match file.read(&mut buf) {
                Ok(0) => Ok(None),
                Ok(_) => Ok(Some(buf[0])),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(e).context("serial read"),
            }
        }
        Err(e) => Err(anyhow!("poll: {e}")),
    }
}

fn write_all(file: &mut File, data: &[u8]) -> Result<()> {
    file.write_all(data).context("serial write")?;
    file.flush().context("serial flush")?;
    Ok(())
}

// ---- framing --------------------------------------------------------------

fn build_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len()).expect("frame length fits in u16");
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update_slice(&len.to_le_bytes());
    crc.update_slice(payload);
    let crc = crc.finalize();

    let mut buf = Vec::with_capacity(8 + payload.len());
    buf.push(SOF);
    buf.push(kind);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(payload);
    buf.extend_from_slice(&crc.to_le_bytes());
    buf
}

/// Read one complete frame, resyncing on SOF and discarding stray bytes.
///
/// Stray bytes are expected at startup (the device may still be flushing a
/// boot log). The proto Parser raises `BadSof` on each non-SOF byte, which
/// we silently swallow until the parser locks on.
fn read_frame(file: &mut File, timeout: Duration) -> Result<(u8, Vec<u8>)> {
    let deadline = Instant::now() + timeout;
    let mut parser = Parser::new();
    loop {
        let Some(byte) = read_byte_until(file, deadline)? else {
            bail!("timed out waiting for frame");
        };
        match parser.step(byte) {
            ProtoPoll::Pending => continue,
            ProtoPoll::Bad(BadReason::BadSof) => continue,
            ProtoPoll::Bad(reason) => bail!("framing error: {reason:?}"),
            ProtoPoll::Frame(f) => return Ok((f.kind, f.payload.to_vec())),
        }
    }
}

fn expect_ack(file: &mut File, what: &str) -> Result<()> {
    let (kind, payload) = read_frame(file, Duration::from_secs(5))
        .with_context(|| format!("waiting for ACK of {what}"))?;
    match kind {
        TYPE_ACK => Ok(()),
        TYPE_NAK => {
            let reason = payload
                .first()
                .copied()
                .map(NakReason::from_byte)
                .unwrap_or(NakReason::Unspecified);
            Err(anyhow!("device NAK'd {what}: {reason}"))
        }
        other => Err(anyhow!(
            "expected ACK for {what}, got type {other:#04x} payload={payload:02x?}"
        )),
    }
}

// ---- upload ---------------------------------------------------------------

fn upload(port_path: &str, bin: &PathBuf, baud: u32) -> Result<()> {
    let raw = std::fs::read(bin).with_context(|| format!("reading {}", bin.display()))?;
    if raw.len() < ExecHeader::SIZE {
        bail!(
            "{}: too small to contain ExecHeader ({} bytes)",
            bin.display(),
            ExecHeader::SIZE,
        );
    }
    let (header_bytes, body) = raw.split_at(ExecHeader::SIZE);
    let header = ExecHeader::parse(header_bytes).expect("split at SIZE");
    let expected_body = header.text_size as usize + header.data_size as usize;
    if body.len() != expected_body {
        bail!(
            "{}: body is {} bytes; header says {}",
            bin.display(),
            body.len(),
            expected_body,
        );
    }

    let mut port = open_serial(port_path, baud)?;

    // Give the kernel a moment to drain any boot log and arrive at the
    // parser. Without this the first SOF can land mid-println.
    std::thread::sleep(Duration::from_millis(100));
    let _ = termios::tcflush(port.as_fd(), termios::FlushArg::TCIOFLUSH);

    write_all(&mut port, &build_frame(TYPE_START_LOAD, header_bytes))?;
    expect_ack(&mut port, "START_LOAD")?;

    for (i, chunk) in body.chunks(CHUNK_MAX).enumerate() {
        write_all(&mut port, &build_frame(TYPE_CHUNK, chunk))?;
        expect_ack(&mut port, &format!("CHUNK@{}", i * CHUNK_MAX))?;
    }

    write_all(&mut port, &build_frame(TYPE_EOF, &[]))?;
    expect_ack(&mut port, "EOF")?;

    let (kind, payload) = read_frame(&mut port, Duration::from_secs(30))?;
    if kind != TYPE_RESULT {
        bail!("expected RESULT, got type {kind:#04x}");
    }
    let r = ResultFrame::parse(&payload)
        .ok_or_else(|| anyhow!("bad RESULT length {}", payload.len()))?;
    print_result(&r);
    std::process::exit(if r.status == STATUS_OK { 0 } else { 2 });
}

fn print_result(r: &ResultFrame) {
    let name = match r.status {
        STATUS_OK => "OK",
        STATUS_TIMEOUT => "TIMEOUT",
        STATUS_MEMFAULT => "MEMFAULT",
        STATUS_BUSFAULT => "BUSFAULT",
        STATUS_USAGEFAULT => "USAGEFAULT",
        STATUS_LOAD_ERROR => "LOAD_ERROR",
        _ => "?",
    };
    let micros = (r.cycles as f64) / (r.cclk_hz as f64) * 1e6;
    println!(
        "status={name} exit={:#010x} cycles={} cclk_hz={} ({micros:.1} us)",
        r.exit_code, r.cycles, r.cclk_hz,
    );
}

// ---- demo .bin -------------------------------------------------------------

// Thumb-2 encoding of `movs r0, #42` (T1 MOV imm): 00100 Rd[10:8] imm8
// = 0010 0000 0010 1010 = 0x202A. LE bytes: 2A 20.
// Thumb `svc #0` (T1 SVC): 1101 1111 imm8 = 0xDF00. LE bytes: 00 DF.
const DEMO_EXIT_42: [u8; 4] = [0x2A, 0x20, 0x00, 0xDF];

fn write_demo_bin(out: &PathBuf) -> Result<()> {
    let header = ExecHeader {
        magic: ExecHeader::MAGIC,
        version: ExecHeader::VERSION,
        flags: 0,
        entry_offset: 0,
        text_size: DEMO_EXIT_42.len() as u32,
        data_size: 0,
        bss_size: 0,
        timeout_ms: 100,
        reserved: 0,
    };
    let mut header_bytes = [0u8; ExecHeader::SIZE];
    header.write_le(&mut header_bytes);

    let mut file = File::create(out).with_context(|| format!("creating {}", out.display()))?;
    file.write_all(&header_bytes)?;
    file.write_all(&DEMO_EXIT_42)?;
    let total = ExecHeader::SIZE + DEMO_EXIT_42.len();
    println!(
        "wrote {} ({total} bytes): runs `movs r0,#42; svc #0`",
        out.display(),
    );
    Ok(())
}
