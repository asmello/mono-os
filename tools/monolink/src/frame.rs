//! Frame build / read / ack helpers shared by the loader.
//!
//! All functions here operate over a [`Transport`] so the loader can drive
//! both POSIX `File`s (via [`FileTransport`](crate::FileTransport)) and
//! caller-supplied serial implementations (e.g. the `serialport` crate from
//! the runner) without code duplication.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use monolink_proto::{
    BadReason, Crc32, NakReason, Parser, Poll as ProtoPoll, SOF, TYPE_ACK, TYPE_NAK,
};

use crate::transport::Transport;

/// Build a complete frame (`SOF | TYPE | LEN | PAYLOAD | CRC32`) into a Vec.
pub(crate) fn build_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
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

/// Write a full frame to the transport and flush.
pub(crate) fn write_frame<T: Transport>(transport: &mut T, frame: &[u8]) -> Result<()> {
    transport.write_all(frame).context("serial write")?;
    transport.flush().context("serial flush")?;
    Ok(())
}

/// Read one complete frame, resyncing on SOF and discarding stray bytes.
///
/// Stray bytes are expected at startup (the device may still be flushing a
/// boot log). The proto Parser raises [`BadReason::BadSof`] on each non-SOF
/// byte, which we silently swallow until the parser locks on.
pub(crate) fn read_frame<T: Transport>(
    transport: &mut T,
    timeout: Duration,
) -> Result<(u8, Vec<u8>)> {
    let deadline = Instant::now() + timeout;
    let mut parser = Parser::new();
    loop {
        let Some(byte) = transport
            .read_byte_with_deadline(deadline)
            .context("serial read")?
        else {
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

/// Read the next frame and require it to be an ACK.
///
/// On NAK, decode the [`NakReason`] payload byte and surface it. Any other
/// frame type is treated as a protocol error.
pub(crate) fn expect_ack<T: Transport>(transport: &mut T, what: &str) -> Result<()> {
    let (kind, payload) = read_frame(transport, Duration::from_secs(5))
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
