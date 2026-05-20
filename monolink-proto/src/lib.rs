//! Shared wire format for monolink v0 — used by both the kernel and the host loader.
//!
//! Pure logic: frame constants, [`ExecHeader`], [`ResultFrame`], [`Crc32`], and
//! the byte-by-byte [`Parser`] state machine. Nothing here touches a UART or
//! a serial port; the embedded side wraps [`Parser::step`] with a per-board
//! ring-buffer read, the host side wraps it with `serialport` reads.
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
//! CRC32 is IEEE 802.3 (the flavor `zlib.crc32` ships) computed over
//! `TYPE || LEN || PAYLOAD`.

#![no_std]

// ---- frame types -----------------------------------------------------------

pub const TYPE_START_LOAD: u8 = 0x01;
pub const TYPE_CHUNK: u8 = 0x02;
pub const TYPE_EOF: u8 = 0x03;
pub const TYPE_CASE: u8 = 0x04;
pub const TYPE_CASE_END: u8 = 0x05;
pub const TYPE_RUN_END: u8 = 0x06;
pub const TYPE_ACK: u8 = 0x10;
pub const TYPE_NAK: u8 = 0x11;
pub const TYPE_RESULT: u8 = 0x20;
pub const TYPE_CASE_OUTPUT: u8 = 0x21;
pub const TYPE_CASE_RESULT: u8 = 0x22;

pub const SOF: u8 = 0x55;

/// Largest CHUNK payload the host sends. Parser headroom is slightly larger
/// (see [`Parser::PAYLOAD_MAX`]) so a START_LOAD-sized header also fits.
pub const CHUNK_MAX: usize = 256;

// ---- NAK reasons -----------------------------------------------------------

/// One-byte payload of a [`TYPE_NAK`] frame.
///
/// The kernel uses these to report exactly why a session was rejected so
/// the host tool can print a useful message without the kernel ever
/// emitting raw text on the wire mid-session (which would interleave with
/// framed bytes and break stricter clients).
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NakReason {
    Unspecified = 0,
    /// Parser saw a payload length larger than [`Parser::PAYLOAD_MAX`].
    FramingOversized = 1,
    /// Frame arrived with a CRC32 that didn't match its body.
    FramingCrcMismatch = 2,
    /// Frame's TYPE byte isn't one the kernel knows.
    UnknownType = 3,
    /// `CHUNK` arrived with no preceding `START_LOAD`.
    ChunkBeforeStart = 4,
    /// `EOF` arrived with no preceding `START_LOAD`.
    EofBeforeStart = 5,
    /// `EOF` arrived before all declared bytes had been streamed.
    EofShort = 6,
    /// `START_LOAD` payload wasn't [`ExecHeader::SIZE`] bytes.
    BadHeaderSize = 7,
    /// Header magic or version didn't match the kernel's expectation.
    BadMagicOrVersion = 8,
    /// `text_size` exceeds the user-text region's capacity.
    LoaderTextTooLarge = 9,
    /// `data_size + bss_size` exceeds the user-data region's size.
    LoaderDataTooLarge = 10,
    /// A chunk wrote past the declared `text_size + data_size`.
    LoaderOverflow = 11,
    /// `CASE` / `CASE_END` arrived before a program was loaded.
    CaseBeforeRun = 12,
    /// `RUN_END` arrived before a program was loaded.
    RunEndBeforeRun = 13,
    /// Accumulated `CASE` payload exceeded the device input buffer.
    CaseInputOverflow = 14,
}

impl NakReason {
    /// Decode a wire byte. Unknown values map to [`NakReason::Unspecified`].
    pub const fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::FramingOversized,
            2 => Self::FramingCrcMismatch,
            3 => Self::UnknownType,
            4 => Self::ChunkBeforeStart,
            5 => Self::EofBeforeStart,
            6 => Self::EofShort,
            7 => Self::BadHeaderSize,
            8 => Self::BadMagicOrVersion,
            9 => Self::LoaderTextTooLarge,
            10 => Self::LoaderDataTooLarge,
            11 => Self::LoaderOverflow,
            12 => Self::CaseBeforeRun,
            13 => Self::RunEndBeforeRun,
            14 => Self::CaseInputOverflow,
            _ => Self::Unspecified,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::FramingOversized => "oversized frame",
            Self::FramingCrcMismatch => "frame CRC mismatch",
            Self::UnknownType => "unknown frame type",
            Self::ChunkBeforeStart => "chunk before start_load",
            Self::EofBeforeStart => "eof before start_load",
            Self::EofShort => "eof before all bytes streamed",
            Self::BadHeaderSize => "start_load payload wasn't header-sized",
            Self::BadMagicOrVersion => "bad exec-header magic or version",
            Self::LoaderTextTooLarge => "text_size exceeds user-text capacity",
            Self::LoaderDataTooLarge => "data_size + bss_size exceeds user-data size",
            Self::LoaderOverflow => "chunk wrote past declared body length",
            Self::CaseBeforeRun => "case frame before program loaded",
            Self::RunEndBeforeRun => "run_end before program loaded",
            Self::CaseInputOverflow => "accumulated case input exceeds device buffer",
        }
    }
}

impl core::fmt::Display for NakReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

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

    /// Serialize into a 32-byte buffer in wire order.
    pub fn write_le(&self, out: &mut [u8; Self::SIZE]) {
        out[0..4].copy_from_slice(&self.magic);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..12].copy_from_slice(&self.entry_offset.to_le_bytes());
        out[12..16].copy_from_slice(&self.text_size.to_le_bytes());
        out[16..20].copy_from_slice(&self.data_size.to_le_bytes());
        out[20..24].copy_from_slice(&self.bss_size.to_le_bytes());
        out[24..28].copy_from_slice(&self.timeout_ms.to_le_bytes());
        out[28..32].copy_from_slice(&self.reserved.to_le_bytes());
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

    /// Decode from a 20-byte little-endian slice.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        let mut pad = [0u8; 3];
        pad.copy_from_slice(&bytes[1..4]);
        Some(Self {
            status: bytes[0],
            _pad: pad,
            exit_code: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            cycles: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            cclk_hz: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        })
    }
}

/// 16-byte payload of a [`TYPE_CASE_RESULT`] frame.
///
/// One per case. Output bytes are *not* inline — they ride on separate
/// [`TYPE_CASE_OUTPUT`] frames sent before this summary, which the host
/// accumulates per-case.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CaseResultFrame {
    pub status: u8,
    pub _pad: [u8; 3],
    pub exit_code: u32,
    pub cycles: u64,
}

impl CaseResultFrame {
    pub const SIZE: usize = 16;

    pub fn write_le(&self, out: &mut [u8; Self::SIZE]) {
        out[0] = self.status;
        out[1..4].copy_from_slice(&self._pad);
        out[4..8].copy_from_slice(&self.exit_code.to_le_bytes());
        out[8..16].copy_from_slice(&self.cycles.to_le_bytes());
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        let mut pad = [0u8; 3];
        pad.copy_from_slice(&bytes[1..4]);
        Some(Self {
            status: bytes[0],
            _pad: pad,
            exit_code: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            cycles: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        })
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

/// Outcome of one [`Parser::step`] call.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Poll<'a> {
    /// Byte consumed; frame still in progress.
    Pending,
    /// A complete frame arrived.
    ///
    /// The slice borrows from the parser's internal buffer and is valid
    /// until the next `step`.
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

    /// Drive the state machine forward by one byte.
    ///
    /// The embedded side calls this after pulling a byte off the per-board
    /// UART ring; the host side calls it after reading from the serial port.
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
