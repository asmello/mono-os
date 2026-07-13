#!/usr/bin/env -S uv run --script
"""Construct a valid AMD64 ELF hello-world executable by hand, without using assembler or linker tools."""
import os
import stat
import struct

# -- layout -------------------------------------------------
LOAD_ADDR = 0x400000
ELF_HDR_SZ = 64
PHDR_SZ = 56
CODE_OFFSET = ELF_HDR_SZ + PHDR_SZ  # 120
ENTRY = LOAD_ADDR + CODE_OFFSET

MSG = b"Hello, world!\n"

# -- x86-64 machine code -----------------------------------
# write(1, msg, len(msg)); exit(0)
#
# lea rsi, [rip+disp32] uses displacement relative to the next instruction.
prefix = bytes([
    0xB8, 0x01, 0x00, 0x00, 0x00,  # mov eax, 1   (sys_write)
    0xBF, 0x01, 0x00, 0x00, 0x00,  # mov edi, 1   (fd = stdout)
])
lea_op = bytes([0x48, 0x8D, 0x35])  # lea rsi, [rip+disp32]
suffix = bytes([
    0xBA, 0x0E, 0x00, 0x00, 0x00,  # mov edx, 14  (byte count)
    0x0F, 0x05,                    # syscall
    0xB8, 0x3C, 0x00, 0x00, 0x00,  # mov eax, 60  (sys_exit)
    0x31, 0xFF,                    # xor edi, edi (status = 0)
    0x0F, 0x05,                    # syscall
])

code_size = len(prefix) + len(lea_op) + 4 + len(suffix)
msg_addr = LOAD_ADDR + CODE_OFFSET + code_size
lea_next_ip = ENTRY + len(prefix) + len(lea_op) + 4
disp32 = msg_addr - lea_next_ip

if not (-(1 << 31) <= disp32 < (1 << 31)):
    raise ValueError(f"RIP-relative displacement out of range: {disp32}")

code = prefix + lea_op + struct.pack("<i", disp32) + suffix
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
    0x3E,        # e_machine: EM_X86_64
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
