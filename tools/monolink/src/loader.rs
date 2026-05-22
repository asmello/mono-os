//! [`Loader`]: drives the upload → case-run → result flow over a [`Transport`].
//!
//! One `Loader` borrows one transport for the lifetime of a single run. To
//! drive a sequence of runs against the same physical device, reuse the
//! transport (drop and recreate the `Loader` between runs is fine — it
//! holds no per-run state).

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use monolink_proto::{
    CHUNK_MAX, CaseResultFrame, ExecHeader, NakReason, ResultFrame, TYPE_CASE, TYPE_CASE_END,
    TYPE_CASE_OUTPUT, TYPE_CASE_RESULT, TYPE_CHUNK, TYPE_EOF, TYPE_NAK, TYPE_RESULT, TYPE_RUN_END,
    TYPE_START_LOAD,
};

use crate::frame::{build_frame, expect_ack, read_frame, write_frame};
use crate::transport::Transport;

/// Pause between opening the transport and sending the first frame.
///
/// Gives the kernel a moment to drain any boot log so the first SOF doesn't
/// land mid-println. Empirically 100 ms is comfortably enough on both QEMU
/// and the LPC1768 over USB-CDC.
const STARTUP_SETTLE: Duration = Duration::from_millis(100);

/// Per-case-result wait window. Case execution is bounded by the
/// kernel-side `timeout_ms` in the header but we add slack for the
/// case-output flush + framing.
const CASE_TIMEOUT: Duration = Duration::from_secs(30);
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Outcome of a full `upload_and_run` invocation.
///
/// Per-case data (status, cycles, output) is delivered through the
/// `on_case` callback as each case completes, so by the time this struct
/// is constructed the caller has already seen every case.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub overall: ResultFrame,
}

/// Stateless driver around a single [`Transport`].
pub struct Loader<T: Transport> {
    transport: T,
}

impl<T: Transport> Loader<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Drive a full upload → multi-case → final-RESULT exchange.
    ///
    /// `bin` is the contents of a monoexec `.bin` (32-byte header followed
    /// by `text_size + data_size` bytes of body). `cases` is the input
    /// payload for each program invocation; pass a single empty `Vec` for
    /// single-shot programs that take no input.
    ///
    /// `on_case` is invoked once per case in order, with the case index, the
    /// device-reported [`CaseResultFrame`], and any `write()` output the
    /// program produced. The callback runs synchronously between cases —
    /// callers that want to stream results elsewhere (e.g. a WebSocket)
    /// should send from inside the callback rather than after the call
    /// returns.
    ///
    /// On the first detected NAK or framing error this returns `Err` with
    /// context naming the phase; partial progress (cases reported via the
    /// callback) stays valid.
    pub fn upload_and_run<F>(
        &mut self,
        bin: &[u8],
        cases: &[Vec<u8>],
        mut on_case: F,
    ) -> Result<RunReport>
    where
        F: FnMut(usize, &CaseResultFrame, &[u8]),
    {
        if bin.len() < ExecHeader::SIZE {
            bail!(
                "bin too small to contain ExecHeader ({} bytes)",
                ExecHeader::SIZE,
            );
        }
        let (header_bytes, body) = bin.split_at(ExecHeader::SIZE);
        let header = ExecHeader::parse(header_bytes).expect("split at SIZE");
        let expected_body = header.text_size as usize + header.data_size as usize;
        if body.len() != expected_body {
            bail!(
                "bin body is {} bytes; header says {}",
                body.len(),
                expected_body,
            );
        }

        // Let any boot-log bytes from the device land before we start framing.
        std::thread::sleep(STARTUP_SETTLE);
        self.transport
            .clear_input()
            .context("draining transport before upload")?;

        // --- upload phase ---
        write_frame(
            &mut self.transport,
            &build_frame(TYPE_START_LOAD, header_bytes),
        )?;
        expect_ack(&mut self.transport, "START_LOAD")?;

        for (i, chunk) in body.chunks(CHUNK_MAX).enumerate() {
            write_frame(&mut self.transport, &build_frame(TYPE_CHUNK, chunk))?;
            expect_ack(&mut self.transport, &format!("CHUNK@{}", i * CHUNK_MAX))?;
        }

        write_frame(&mut self.transport, &build_frame(TYPE_EOF, &[]))?;
        expect_ack(&mut self.transport, "EOF")?;

        // --- case phase ---
        for (idx, case) in cases.iter().enumerate() {
            for (i, chunk) in case.chunks(CHUNK_MAX).enumerate() {
                write_frame(&mut self.transport, &build_frame(TYPE_CASE, chunk))?;
                expect_ack(
                    &mut self.transport,
                    &format!("CASE[{idx}]@{}", i * CHUNK_MAX),
                )?;
            }
            write_frame(&mut self.transport, &build_frame(TYPE_CASE_END, &[]))?;
            let (cr, output) = read_case_result(&mut self.transport)?;
            on_case(idx, &cr, &output);
        }

        write_frame(&mut self.transport, &build_frame(TYPE_RUN_END, &[]))?;

        // --- final result ---
        let (kind, payload) = read_frame(&mut self.transport, FINAL_RESULT_TIMEOUT)?;
        if kind != TYPE_RESULT {
            bail!("expected final RESULT, got type {kind:#04x}");
        }
        let overall = ResultFrame::parse(&payload)
            .ok_or_else(|| anyhow!("bad RESULT length {}", payload.len()))?;
        Ok(RunReport { overall })
    }
}

/// Read frames until a `CASE_RESULT` arrives, accumulating any
/// `CASE_OUTPUT` chunks along the way.
fn read_case_result<T: Transport>(transport: &mut T) -> Result<(CaseResultFrame, Vec<u8>)> {
    let mut output = Vec::new();
    loop {
        let (kind, payload) = read_frame(transport, CASE_TIMEOUT)?;
        match kind {
            TYPE_CASE_OUTPUT => output.extend_from_slice(&payload),
            TYPE_CASE_RESULT => {
                let cr = CaseResultFrame::parse(&payload)
                    .ok_or_else(|| anyhow!("bad CASE_RESULT length {}", payload.len()))?;
                return Ok((cr, output));
            }
            TYPE_NAK => {
                let reason = payload
                    .first()
                    .copied()
                    .map(NakReason::from_byte)
                    .unwrap_or(NakReason::Unspecified);
                bail!("device NAK'd during case: {reason}");
            }
            other => bail!("unexpected frame type {other:#04x} during case"),
        }
    }
}
