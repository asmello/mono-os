#!/usr/bin/env -S uv run --script
"""Construct a valid ARM64 ELF hello-world executable by hand, without using assembler or linker tools."""
import os
import stat
import struct

# -- layout -------------------------------------------------
LOAD_ADDR = 0x400000
ELF_HDR_SZ = 64
PHDR_SZ = 56
CODE_OFFSET = ELF_HDR_SZ + PHDR_SZ  # 120
ENTRY = LOAD_ADDR + CODE_OFFSET

# Program layout inside the load segment:
#   [code (32 bytes)] [message bytes]
MSG = b"Hello, world!\n"
CODE_SIZE = 8 * 4  # 8 AArch64 instructions, 4 bytes each
MSG_OFFSET = CODE_OFFSET + CODE_SIZE
MSG_ADDR = LOAD_ADDR + MSG_OFFSET

# adr x1, msg is at instruction offset +8 from ENTRY.
# ADR uses signed 21-bit PC-relative immediate with bit split:
#   immlo = imm[1:0] at bits [30:29], immhi = imm[20:2] at bits [23:5]
adr_pc = ENTRY + 8
adr_imm = MSG_ADDR - adr_pc
if not (-(1 << 20) <= adr_imm < (1 << 20)):
    raise ValueError(f"ADR offset out of range: {adr_imm}")

imm21 = adr_imm & ((1 << 21) - 1)
immlo = imm21 & 0x3
immhi = (imm21 >> 2) & 0x7FFFF
adr_x1_msg = 0x10000000 | (immlo << 29) | (immhi << 5) | 1

# -- AArch64 machine code ----------------------------------
# write(1, msg, len(msg)); exit(0)
code_words = [
    0xD2800808,  # mov x8, #64   (sys_write)
    0xD2800020,  # mov x0, #1    (fd = stdout)
    adr_x1_msg,  # adr x1, msg   (buffer pointer)
    0xD28001C2,  # mov x2, #14   (byte count)
    0xD4000001,  # svc #0
    0xD2800BA8,  # mov x8, #93   (sys_exit)
    0xD2800000,  # mov x0, #0    (status = 0)
    0xD4000001,  # svc #0
]
code = b"".join(struct.pack("<I", w) for w in code_words)

FILE_SIZE = CODE_OFFSET + len(code) + len(MSG)

# -- ELF header part 1: e_ident ----------------------------
ident = struct.pack(
    "4sBBBBB7x",
    b"\x7fELF",  # magic
    2,            # EI_CLASS: 64-bit
    1,            # EI_DATA: little-endian
    1,            # EI_VERSION
    0,            # EI_OSABI: System V
    0,            # EI_ABIVERSION
)

# -- ELF header part 2 -------------------------------------
ehdr = struct.pack(
    "<HHIQQQIHHHHHH",
    2,           # e_type: ET_EXEC
    0xB7,        # e_machine: EM_AARCH64
    1,           # e_version
    ENTRY,       # e_entry
    ELF_HDR_SZ,  # e_phoff
    0,           # e_shoff
    0,           # e_flags
    ELF_HDR_SZ,  # e_ehsize
    PHDR_SZ,     # e_phentsize
    1,           # e_phnum
    64,          # e_shentsize
    0,           # e_shnum
    0,           # e_shstrndx
)

# -- Program header ----------------------------------------
phdr = struct.pack(
    "<IIQQQQQQ",
    1,         # p_type: PT_LOAD
    0x5,       # p_flags: PF_R | PF_X
    0,         # p_offset
    LOAD_ADDR, # p_vaddr
    LOAD_ADDR, # p_paddr
    FILE_SIZE, # p_filesz
    FILE_SIZE, # p_memsz
    0x200000,  # p_align
)

# -- write file --------------------------------------------
binary = ident + ehdr + phdr + code + MSG
assert len(ident) + len(ehdr) == ELF_HDR_SZ
assert len(phdr) == PHDR_SZ
assert len(binary) == FILE_SIZE

with open("hello0", "wb") as f:
    f.write(binary)

os.chmod("hello0", os.stat("hello0").st_mode | stat.S_IEXEC)
print(f"Written {len(binary)} bytes")
print(f"Entry point: {hex(ENTRY)}")
