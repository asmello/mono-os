#!/usr/bin/env python3
"""monolink: ship a user-program .bin to the device, wait for the RESULT.

Usage:
    tools/monolink.py upload [--baud RATE] PORT BIN
    tools/monolink.py demo OUT      # write a tiny exit(42) .bin for sanity tests

A .bin is just the 32-byte ExecHeader followed by the text image and
the data init image (concatenated, no padding). The packaging crate
(M3) builds these from a user-side Rust program; for v1 we hand-roll
one in the `demo` subcommand so the dev loop is self-contained.

Wire framing (matches kernel/src/link.rs):
    SOF 0x55 | TYPE u8 | LEN u16 LE | PAYLOAD[LEN] | CRC32 u32 LE

CRC32 is IEEE 802.3 (zlib.crc32) over TYPE || LEN || PAYLOAD.

POSIX-only. Uses termios directly; no pyserial dependency.
"""

from __future__ import annotations

import argparse
import os
import struct
import sys
import termios
import time
import zlib
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

# Frame types — keep in lockstep with kernel/src/link.rs.
TYPE_START_LOAD = 0x01
TYPE_CHUNK = 0x02
TYPE_EOF = 0x03
TYPE_ACK = 0x10
TYPE_NAK = 0x11
TYPE_RESULT = 0x20

SOF = 0x55
CHUNK_MAX = 256

STATUS_NAMES = {
    0: "OK",
    1: "TIMEOUT",
    2: "MEMFAULT",
    3: "BUSFAULT",
    4: "USAGEFAULT",
    5: "LOAD_ERROR",
}


def build_frame(kind: int, payload: bytes) -> bytes:
    header = bytes([SOF, kind]) + struct.pack("<H", len(payload))
    crc_in = bytes([kind]) + struct.pack("<H", len(payload)) + payload
    crc = zlib.crc32(crc_in) & 0xFFFFFFFF
    return header + payload + struct.pack("<I", crc)


def read_exact(fd: int, n: int, deadline: float) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(f"timed out reading {n} bytes (got {len(buf)})")
        try:
            chunk = os.read(fd, n - len(buf))
        except BlockingIOError:
            time.sleep(0.005)
            continue
        if not chunk:
            time.sleep(0.005)
            continue
        buf.extend(chunk)
    return bytes(buf)


def read_frame(fd: int, timeout_s: float = 5.0) -> tuple[int, bytes]:
    deadline = time.monotonic() + timeout_s
    # Resync on SOF — the device might still be flushing log lines.
    while True:
        b = read_exact(fd, 1, deadline)
        if b[0] == SOF:
            break
    kind = read_exact(fd, 1, deadline)[0]
    length = struct.unpack("<H", read_exact(fd, 2, deadline))[0]
    payload = read_exact(fd, length, deadline)
    crc_bytes = read_exact(fd, 4, deadline)
    crc_recv = struct.unpack("<I", crc_bytes)[0]
    crc_calc = zlib.crc32(bytes([kind]) + struct.pack("<H", length) + payload) & 0xFFFFFFFF
    if crc_recv != crc_calc:
        raise ValueError(f"crc mismatch: recv={crc_recv:#010x} calc={crc_calc:#010x}")
    return kind, payload


@contextmanager
def open_serial(port: str, baud: int):
    """Open `port` raw at `baud`, restore tty state on exit."""
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        saved = termios.tcgetattr(fd)
        try:
            attrs = termios.tcgetattr(fd)
            # iflag, oflag, cflag, lflag, ispeed, ospeed, cc
            iflag, oflag, cflag, lflag, _, _, cc = attrs
            iflag = 0
            oflag = 0
            lflag = 0
            # 8N1, enable receiver, ignore modem ctrl lines, no flow ctrl.
            cflag = (cflag & ~termios.CSIZE) | termios.CS8 | termios.CREAD | termios.CLOCAL
            cflag &= ~(termios.PARENB | termios.CSTOPB | termios.CRTSCTS)
            cc = list(cc)
            cc[termios.VMIN] = 0
            cc[termios.VTIME] = 0
            termios.tcsetattr(
                fd,
                termios.TCSANOW,
                [iflag, oflag, cflag, lflag, baud, baud, cc],
            )
            # Belt-and-braces: also set the speed via the dedicated ioctl
            # if the system supports custom baud rates. On macOS the speeds
            # above field above are honored directly.
            termios.tcflush(fd, termios.TCIOFLUSH)
            yield fd
        finally:
            termios.tcsetattr(fd, termios.TCSANOW, saved)
    finally:
        os.close(fd)


def baud_value(rate: int) -> int:
    """Return termios speed constant for `rate`, or `rate` itself if the
    OS accepts arbitrary integers (Linux, macOS)."""
    sym = f"B{rate}"
    if hasattr(termios, sym):
        return getattr(termios, sym)
    return rate


def write_all(fd: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        try:
            n = os.write(fd, view)
        except BlockingIOError:
            time.sleep(0.001)
            continue
        view = view[n:]


def expect_ack(fd: int, what: str) -> None:
    kind, payload = read_frame(fd)
    if kind == TYPE_ACK:
        return
    if kind == TYPE_NAK:
        raise RuntimeError(f"device NAK'd {what}")
    raise RuntimeError(f"expected ACK for {what}, got type {kind:#04x} payload={payload!r}")


@dataclass
class ResultFrame:
    status: int
    exit_code: int
    cycles: int
    cclk_hz: int

    @classmethod
    def parse(cls, payload: bytes) -> "ResultFrame":
        if len(payload) != 20:
            raise ValueError(f"bad RESULT length {len(payload)}")
        status = payload[0]
        exit_code, cycles, cclk_hz = struct.unpack("<I Q I", payload[4:20])
        return cls(status, exit_code, cycles, cclk_hz)


def upload(port: str, bin_path: Path, baud: int) -> int:
    raw = bin_path.read_bytes()
    if len(raw) < 32:
        raise SystemExit(f"{bin_path}: too small to contain ExecHeader (32 bytes)")
    header = raw[:32]
    body = raw[32:]
    text_size, data_size = struct.unpack_from("<II", header, 12)
    expected_body = text_size + data_size
    if len(body) != expected_body:
        raise SystemExit(
            f"{bin_path}: body is {len(body)} bytes; header says {expected_body}"
        )

    with open_serial(port, baud_value(baud)) as fd:
        # Give the kernel a moment to drain any boot log and arrive at the
        # parser. Without this the first SOF can land mid-println.
        time.sleep(0.1)
        termios.tcflush(fd, termios.TCIOFLUSH)

        write_all(fd, build_frame(TYPE_START_LOAD, header))
        expect_ack(fd, "START_LOAD")

        for i in range(0, len(body), CHUNK_MAX):
            chunk = body[i : i + CHUNK_MAX]
            write_all(fd, build_frame(TYPE_CHUNK, chunk))
            expect_ack(fd, f"CHUNK@{i}")

        write_all(fd, build_frame(TYPE_EOF, b""))
        expect_ack(fd, "EOF")

        kind, payload = read_frame(fd, timeout_s=30.0)
        if kind != TYPE_RESULT:
            raise SystemExit(f"expected RESULT, got type {kind:#04x}")
        r = ResultFrame.parse(payload)

    name = STATUS_NAMES.get(r.status, f"?{r.status}")
    print(
        f"status={name} exit={r.exit_code:#010x} cycles={r.cycles} "
        f"cclk_hz={r.cclk_hz} ({r.cycles / r.cclk_hz * 1e6:.1f} us)"
    )
    return 0 if r.status == 0 else 2


# ---- demo .bin synthesis ----------------------------------------------------

# Thumb-2 encoding of `movs r0, #42` (T1 MOV immediate): 00100 Rd[10:8] imm8
# = 0010 0000 0010 1010 = 0x202A. LE bytes: 2A 20.
# Thumb `svc #0` (T1 SVC): 1101 1111 imm8 = 0xDF00. LE bytes: 00 DF.
DEMO_EXIT_42 = bytes([0x2A, 0x20, 0x00, 0xDF])


def make_demo_bin() -> bytes:
    text = DEMO_EXIT_42
    header = (
        b"MOXE"
        + struct.pack(
            "<HH IIIIII",
            0,  # version
            0,  # flags
            0,  # entry_offset
            len(text),  # text_size
            0,  # data_size
            0,  # bss_size
            100,  # timeout_ms
            0,  # reserved
        )
    )
    assert len(header) == 32, f"header is {len(header)} bytes"
    return header + text


def cmd_demo(args: argparse.Namespace) -> int:
    out = Path(args.out)
    out.write_bytes(make_demo_bin())
    print(f"wrote {out} ({out.stat().st_size} bytes): runs `movs r0,#42; svc #0`")
    return 0


def cmd_upload(args: argparse.Namespace) -> int:
    return upload(args.port, Path(args.bin), args.baud)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="monolink host tool")
    sub = p.add_subparsers(dest="command", required=True)

    up = sub.add_parser("upload", help="send a .bin to the device, print RESULT")
    up.add_argument("port", help="serial device (e.g. /dev/cu.usbmodem* or PTY)")
    up.add_argument("bin", help="path to a monoexec .bin (header + body)")
    up.add_argument("--baud", type=int, default=115200)
    up.set_defaults(func=cmd_upload)

    demo = sub.add_parser("demo", help="write a tiny exit(42) .bin for sanity tests")
    demo.add_argument("out", help="output path")
    demo.set_defaults(func=cmd_demo)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
