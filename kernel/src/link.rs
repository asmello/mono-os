//! `monolink v0`: stop-and-wait UART framing for one user-program image.
//!
//! Ships a single binary from the host to the device, one frame at a time.
//!
//! Wire format (little-endian throughout):
//!
//! ```text
//!     +-------+------+--------+------------------+----------+
//!     |  SOF  | TYPE |  LEN   |     PAYLOAD      |  CRC32   |
//!     | 0x55  |  u8  |  u16   |   LEN bytes      |   u32    |
//!     +-------+------+--------+------------------+----------+
//! ```
//!
//! The CRC32 is over `TYPE || LEN || PAYLOAD` (IEEE 802.3 polynomial,
//! init 0xFFFFFFFF, final XOR 0xFFFFFFFF) — the same flavor `zlib` ships.
//!
//! The receiver is a small state machine driven byte-by-byte from the
//! per-board UART ring buffer ([`crate::board::UartRx::read_byte`]). It
//! buffers one frame at a time into [`Parser::PAYLOAD_MAX`] bytes; longer
//! payloads are rejected with a length error so the host knows to retry.
//!
//! Caller loop:
//!
//! ```ignore
//! let mut parser = link::Parser::new();
//! loop {
//!     match parser.poll() {
//!         link::Poll::Pending => cortex_m::asm::wfi(),
//!         link::Poll::Bad(_) => link::send(link::Frame::nak()),
//!         link::Poll::Frame(f) => handle(f),
//!     }
//! }
//! ```

use crate::board::{Board, Mcu, UartRx, UartTx};

// ---- frame types -----------------------------------------------------------

pub const TYPE_START_LOAD: u8 = 0x01;
pub const TYPE_CHUNK: u8 = 0x02;
pub const TYPE_EOF: u8 = 0x03;
pub const TYPE_ACK: u8 = 0x10;
pub const TYPE_NAK: u8 = 0x11;
pub const TYPE_RESULT: u8 = 0x20;

const SOF: u8 = 0x55;

// ---- ExecHeader ------------------------------------------------------------

/// 32-byte header carried by [`TYPE_START_LOAD`] frames.
///
/// Pinned layout — any field renaming or reorder is a wire-breaking change.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ExecHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub flags: u16,
    pub entry_offset: u32,
    pub text_size: u32,
    pub data_size: u32,
    pub bss_size: u32,
    pub timeout_ms: u32,
    pub reserved: u32,
}

impl ExecHeader {
    pub const SIZE: usize = 32;
    pub const MAGIC: [u8; 4] = *b"MOXE";
    pub const VERSION: u16 = 0;

    /// Decode from a 32-byte little-endian slice.
    ///
    /// Returns `None` only on a size mismatch; field-value validation is
    /// the caller's problem.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[0..4]);
        Some(Self {
            magic,
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
            entry_offset: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            text_size: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            data_size: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            bss_size: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            timeout_ms: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            reserved: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        })
    }
}

// ---- result frame ----------------------------------------------------------

pub const STATUS_OK: u8 = 0;
pub const STATUS_TIMEOUT: u8 = 1;
pub const STATUS_MEMFAULT: u8 = 2;
pub const STATUS_BUSFAULT: u8 = 3;
pub const STATUS_USAGEFAULT: u8 = 4;
pub const STATUS_LOAD_ERROR: u8 = 5;

/// 20-byte payload of a [`TYPE_RESULT`] frame.
///
/// Same pin-layout rules as [`ExecHeader`].
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ResultFrame {
    pub status: u8,
    pub _pad: [u8; 3],
    pub exit_code: u32,
    pub cycles: u64,
    pub cclk_hz: u32,
}

impl ResultFrame {
    pub const SIZE: usize = 20;

    pub fn write_le(&self, out: &mut [u8; Self::SIZE]) {
        out[0] = self.status;
        out[1..4].copy_from_slice(&self._pad);
        out[4..8].copy_from_slice(&self.exit_code.to_le_bytes());
        out[8..16].copy_from_slice(&self.cycles.to_le_bytes());
        out[16..20].copy_from_slice(&self.cclk_hz.to_le_bytes());
    }
}

// ---- CRC32 -----------------------------------------------------------------

/// IEEE 802.3 / zlib CRC32.
///
/// Initialized with `0xFFFFFFFF` and XORed with the same constant on
/// finalization — matches what most host-side tooling does by default
/// (`zlib.crc32`, `crc32` in PySerial scripts, etc).
pub struct Crc32(u32);

impl Crc32 {
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn update(&mut self, byte: u8) {
        // Slice-by-1 with a 256-entry table. Table is generated at compile
        // time so it lives in `.rodata` (1 KiB), not RAM.
        self.0 = (self.0 >> 8) ^ CRC32_TABLE[((self.0 ^ byte as u32) & 0xFF) as usize];
    }

    pub fn update_slice(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.update(b);
        }
    }

    pub fn finalize(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

const CRC32_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

// ---- parser ----------------------------------------------------------------

/// Outcome of one [`Parser::poll`] call.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Poll<'a> {
    /// Ring is empty; no new frame this round.
    Pending,
    /// A complete frame arrived.
    ///
    /// The slice borrows from the parser's internal buffer and is valid
    /// until the next `poll`.
    Frame(Frame<'a>),
    /// A byte was consumed but the frame was malformed.
    ///
    /// Bad SOF, oversized payload, or CRC mismatch. Caller may NAK and
    /// keep polling.
    Bad(BadReason),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Frame<'a> {
    pub kind: u8,
    pub payload: &'a [u8],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BadReason {
    BadSof,
    Oversized,
    CrcMismatch,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    Sof,
    Type,
    Len0,
    Len1,
    Payload,
    Crc0,
    Crc1,
    Crc2,
    Crc3,
}

pub struct Parser {
    state: State,
    kind: u8,
    len: u16,
    payload_idx: u16,
    payload: [u8; Self::PAYLOAD_MAX],
    crc_bytes: [u8; 4],
    crc_idx: u8,
}

impl Parser {
    /// Largest payload we accept.
    ///
    /// Sized for `CHUNK` (256-byte body) with a little headroom. Anything
    /// bigger trips [`BadReason::Oversized`].
    pub const PAYLOAD_MAX: usize = 320;

    pub const fn new() -> Self {
        Self {
            state: State::Sof,
            kind: 0,
            len: 0,
            payload_idx: 0,
            payload: [0; Self::PAYLOAD_MAX],
            crc_bytes: [0; 4],
            crc_idx: 0,
        }
    }

    fn reset(&mut self) {
        self.state = State::Sof;
        self.kind = 0;
        self.len = 0;
        self.payload_idx = 0;
        self.crc_idx = 0;
    }

    /// Consume one byte from the per-board RX ring (if any) and step the state machine.
    ///
    /// Returns the new parser state.
    pub fn poll(&mut self) -> Poll<'_> {
        let byte = match <<Mcu as Board>::Rx as UartRx>::read_byte() {
            Some(b) => b,
            None => return Poll::Pending,
        };
        self.step(byte)
    }

    /// Pure-function step: drive the state machine forward by one byte.
    ///
    /// Exposed so tests can feed bytes directly without going through the
    /// UART ring; the production path is [`Self::poll`].
    pub fn step(&mut self, byte: u8) -> Poll<'_> {
        match self.state {
            State::Sof => {
                if byte != SOF {
                    return Poll::Bad(BadReason::BadSof);
                }
                self.state = State::Type;
                Poll::Pending
            }
            State::Type => {
                self.kind = byte;
                self.state = State::Len0;
                Poll::Pending
            }
            State::Len0 => {
                self.len = byte as u16;
                self.state = State::Len1;
                Poll::Pending
            }
            State::Len1 => {
                self.len |= (byte as u16) << 8;
                if self.len as usize > Self::PAYLOAD_MAX {
                    self.reset();
                    return Poll::Bad(BadReason::Oversized);
                }
                self.state = if self.len == 0 {
                    State::Crc0
                } else {
                    State::Payload
                };
                Poll::Pending
            }
            State::Payload => {
                self.payload[self.payload_idx as usize] = byte;
                self.payload_idx += 1;
                if self.payload_idx == self.len {
                    self.state = State::Crc0;
                }
                Poll::Pending
            }
            State::Crc0 | State::Crc1 | State::Crc2 => {
                self.crc_bytes[self.crc_idx as usize] = byte;
                self.crc_idx += 1;
                self.state = match self.state {
                    State::Crc0 => State::Crc1,
                    State::Crc1 => State::Crc2,
                    State::Crc2 => State::Crc3,
                    _ => unreachable!(),
                };
                Poll::Pending
            }
            State::Crc3 => {
                self.crc_bytes[3] = byte;
                let received = u32::from_le_bytes(self.crc_bytes);
                let mut crc = Crc32::new();
                crc.update(self.kind);
                crc.update_slice(&self.len.to_le_bytes());
                crc.update_slice(&self.payload[..self.len as usize]);
                let computed = crc.finalize();
                let result = if computed == received {
                    Poll::Frame(Frame {
                        kind: self.kind,
                        payload: &self.payload[..self.len as usize],
                    })
                } else {
                    Poll::Bad(BadReason::CrcMismatch)
                };
                // Don't reset before constructing `result` — `Frame` borrows
                // `self.payload`. Reset state-only fields after the borrow
                // is released by the return.
                self.state = State::Sof;
                self.kind = 0;
                self.len = 0;
                self.payload_idx = 0;
                self.crc_idx = 0;
                result
            }
        }
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

// ---- sender ----------------------------------------------------------------

/// Send a complete frame over the console UART.
///
/// Synchronous, blocking on each byte (the LPC1768 driver polls THRE).
pub fn send(kind: u8, payload: &[u8]) {
    let mut uart = <Mcu as Board>::Tx::default();
    let len = payload.len() as u16;
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update_slice(&len.to_le_bytes());
    crc.update_slice(payload);
    let crc = crc.finalize();

    uart.write_byte(SOF);
    uart.write_byte(kind);
    for b in len.to_le_bytes() {
        uart.write_byte(b);
    }
    for &b in payload {
        uart.write_byte(b);
    }
    for b in crc.to_le_bytes() {
        uart.write_byte(b);
    }
}

pub fn send_ack() {
    send(TYPE_ACK, &[]);
}

pub fn send_nak() {
    send(TYPE_NAK, &[]);
}
