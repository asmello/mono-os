//! Pure-byte tests for the monolink frame parser and CRC32 helper.
//!
//! These don't touch the UART at all — they feed bytes through
//! [`kernel::link::Parser::step`] directly. That keeps the test fast and
//! lets us cover the error paths (CRC mismatch, oversized payload, bad
//! SOF) without needing a stub serial port.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;
use kernel::link::{BadReason, Crc32, ExecHeader, Parser, Poll, TYPE_CHUNK, TYPE_START_LOAD};

#[entry]
fn main() -> ! {
    kernel::init();
    test_main();
    kernel::wfi_loop();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

/// Compose a frame in `out` and return the number of bytes written.
/// Format matches what the kernel sender produces.
fn build_frame(kind: u8, payload: &[u8], out: &mut [u8]) -> usize {
    let mut crc = Crc32::new();
    crc.update(kind);
    let len = payload.len() as u16;
    crc.update_slice(&len.to_le_bytes());
    crc.update_slice(payload);
    let crc = crc.finalize();

    out[0] = 0x55;
    out[1] = kind;
    out[2..4].copy_from_slice(&len.to_le_bytes());
    out[4..4 + payload.len()].copy_from_slice(payload);
    out[4 + payload.len()..8 + payload.len()].copy_from_slice(&crc.to_le_bytes());
    8 + payload.len()
}

#[test_case]
fn crc32_known_vector() {
    // zlib.crc32(b"123456789") == 0xCBF43926 — the canonical test vector.
    let mut c = Crc32::new();
    c.update_slice(b"123456789");
    assert_eq!(c.finalize(), 0xCBF4_3926);
}

#[test_case]
fn parser_decodes_short_chunk() {
    let body = b"hello";
    let mut buf = [0u8; 32];
    let n = build_frame(TYPE_CHUNK, body, &mut buf);
    let mut parser = Parser::new();
    // Feed all but the last byte; nothing complete should come out yet.
    for &b in &buf[..n - 1] {
        match parser.step(b) {
            Poll::Pending => {}
            Poll::Frame(_) => panic!("frame too early"),
            Poll::Bad(r) => panic!("unexpected bad: {r:?}"),
        }
    }
    match parser.step(buf[n - 1]) {
        Poll::Frame(f) => {
            assert_eq!(f.kind, TYPE_CHUNK);
            assert_eq!(f.payload, body);
        }
        other => panic!("expected frame, got {other:?}"),
    }
}

#[test_case]
fn parser_rejects_bad_sof() {
    let mut parser = Parser::new();
    match parser.step(0xAB) {
        Poll::Bad(BadReason::BadSof) => {}
        other => panic!("expected BadSof, got {other:?}"),
    }
}

#[test_case]
fn parser_flags_crc_mismatch() {
    let mut buf = [0u8; 16];
    let n = build_frame(TYPE_CHUNK, b"abc", &mut buf);
    // Corrupt the payload after CRC was computed; CRC should disagree.
    buf[4] ^= 0xFF;
    let mut parser = Parser::new();
    let mut last = Poll::Pending;
    for &b in &buf[..n] {
        last = parser.step(b);
    }
    matches!(last, Poll::Bad(BadReason::CrcMismatch))
        .then_some(())
        .unwrap_or_else(|| panic!("expected CrcMismatch, got {last:?}"));
}

#[test_case]
fn parser_rejects_oversized_len() {
    let mut parser = Parser::new();
    assert!(matches!(parser.step(0x55), Poll::Pending));
    assert!(matches!(parser.step(TYPE_CHUNK), Poll::Pending));
    // Declare a 0xFFFF-byte payload — well past PAYLOAD_MAX.
    assert!(matches!(parser.step(0xFF), Poll::Pending));
    match parser.step(0xFF) {
        Poll::Bad(BadReason::Oversized) => {}
        other => panic!("expected Oversized, got {other:?}"),
    }
}

#[test_case]
fn exec_header_roundtrip() {
    // Build a header by hand and parse it back.
    let mut buf = [0u8; ExecHeader::SIZE];
    buf[0..4].copy_from_slice(&ExecHeader::MAGIC);
    buf[4..6].copy_from_slice(&0u16.to_le_bytes()); // version
    buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // flags
    buf[8..12].copy_from_slice(&12u32.to_le_bytes()); // entry_offset
    buf[12..16].copy_from_slice(&0x800u32.to_le_bytes()); // text_size
    buf[16..20].copy_from_slice(&0x100u32.to_le_bytes()); // data_size
    buf[20..24].copy_from_slice(&0x200u32.to_le_bytes()); // bss_size
    buf[24..28].copy_from_slice(&100u32.to_le_bytes()); // timeout_ms
    buf[28..32].copy_from_slice(&0u32.to_le_bytes()); // reserved

    let h = ExecHeader::parse(&buf).expect("32-byte slice");
    assert_eq!(h.magic, ExecHeader::MAGIC);
    assert_eq!(h.version, ExecHeader::VERSION);
    assert_eq!(h.entry_offset, 12);
    assert_eq!(h.text_size, 0x800);
    assert_eq!(h.timeout_ms, 100);
}

#[test_case]
fn parser_decodes_start_load_header_carrying_frame() {
    let mut header = [0u8; ExecHeader::SIZE];
    header[0..4].copy_from_slice(&ExecHeader::MAGIC);
    header[12..16].copy_from_slice(&0x800u32.to_le_bytes());

    let mut frame = [0u8; 64];
    let n = build_frame(TYPE_START_LOAD, &header, &mut frame);

    let mut parser = Parser::new();
    let mut got = None;
    for &b in &frame[..n] {
        if let Poll::Frame(f) = parser.step(b) {
            got = Some((f.kind, f.payload.len()));
            assert_eq!(f.kind, TYPE_START_LOAD);
            let h = ExecHeader::parse(f.payload).expect("32-byte header");
            assert_eq!(h.text_size, 0x800);
        }
    }
    assert!(got.is_some(), "no frame produced");
}
