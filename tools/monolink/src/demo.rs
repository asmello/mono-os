//! Hand-crafted minimal monoexec `.bin` for sanity tests.
//!
//! Two Thumb-2 instructions: `movs r0, #42; svc #0`. The kernel sees `svc #0`
//! (the `exit` syscall) with `R0=42` and reports `status=OK exit=42`.

use monolink_proto::ExecHeader;

// Thumb-2 encoding of `movs r0, #42` (T1 MOV imm): 00100 Rd[10:8] imm8
// = 0010 0000 0010 1010 = 0x202A. LE bytes: 2A 20.
// Thumb `svc #0` (T1 SVC): 1101 1111 imm8 = 0xDF00. LE bytes: 00 DF.
const DEMO_EXIT_42: [u8; 4] = [0x2A, 0x20, 0x00, 0xDF];

/// Build the exit-42 demo binary (header + body) into a fresh `Vec`.
pub fn build_exit42() -> Vec<u8> {
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

    let mut buf = Vec::with_capacity(ExecHeader::SIZE + DEMO_EXIT_42.len());
    buf.extend_from_slice(&header_bytes);
    buf.extend_from_slice(&DEMO_EXIT_42);
    buf
}
