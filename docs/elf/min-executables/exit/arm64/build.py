#!/usr/bin/env -S uv run --script
"""Construct a valid ARM64 ELF executable by hand, without using any assembler or linker tools."""
import struct, os, stat  # noqa: E401

# ── layout ──────────────────────────────────────────
LOAD_ADDR   = 0x400000  # conventional base address for Linux executables
ELF_HDR_SZ  = 64        # ELF header is always 64 bytes on 64-bit
PHDR_SZ     = 56        # program header entry is always 56 bytes on 64-bit
CODE_OFFSET = ELF_HDR_SZ + PHDR_SZ   # 120 — code starts right after both headers
ENTRY       = LOAD_ADDR + CODE_OFFSET # virtual address the kernel jumps to

# ── AArch64 machine code ─────────────────────────────
# AArch64 instructions are always 4 bytes, little-endian encoded.
# Syscall convention: number in x8, args in x0-x5, invoke with svc #0.
# (x86-64 uses rax/rdi/rsi and the 'syscall' instruction instead.)
code = bytes([
    0xA8, 0x0B, 0x80, 0xD2,  # mov x8, #93   — load sys_exit (93) into x8
                              #   (x86-64 equivalent: mov eax, 60)
    0x40, 0x05, 0x80, 0xD2,  # mov x0, #42   — exit status 42 in x0 (first arg)
                              #   (x86-64 equivalent: mov edi, 42)
    0x01, 0x00, 0x00, 0xD4,  # svc #0        — software interrupt: enter kernel
                              #   (x86-64 equivalent: syscall)
])

FILE_SIZE = CODE_OFFSET + len(code)  # total file size: 120 + 12 = 132 bytes

# ── ELF header — part 1: e_ident (16 bytes) ─────────
# e_ident is a fixed 16-byte magic block that identifies the file.
# It's architecture-independent — the same on x86, ARM, RISC-V, etc.
ident = struct.pack('4sBBBBB7x',
    b'\x7fELF',  # EI_MAG:     magic number — loader rejects file if wrong
    2,           # EI_CLASS:   2 = ELFCLASS64 (64-bit addresses)
    1,           # EI_DATA:    1 = ELFDATA2LSB (little-endian)
                 #             2 would be big-endian (e.g. SPARC, old MIPS)
    1,           # EI_VERSION: must be 1 (EV_CURRENT)
    0,           # EI_OSABI:   0 = System V / Linux. 3 = Linux, 9 = FreeBSD
                 #             In practice Linux ignores this field
    0,           # EI_ABIVERSION: usually 0, meaning unspecified
                 # 7x: 7 bytes of padding to bring e_ident to 16 bytes
)

# ── ELF header — part 2: remaining fields (48 bytes) ─
# Format string: < = little-endian
#   H = u16, I = u32, Q = u64
ehdr = struct.pack('<HHIQQQIHHHHHH',
    2,            # e_type:      2 = ET_EXEC (executable)
                  #              3 = ET_DYN  (shared object / PIE binary)
    0xb7,         # e_machine:   0xb7 = EM_AARCH64
                  #              (x86-64 version used 0x3e = EM_X86_64)
    1,            # e_version:   must be 1
    ENTRY,        # e_entry:     virtual address kernel transfers control to
                  #              (0x400078 in our case)
    ELF_HDR_SZ,   # e_phoff:     file offset of program header table (= 64)
                  #              starts immediately after this header
    0,            # e_shoff:     file offset of section header table
                  #              0 = no section headers (loader doesn't need them)
    0,            # e_flags:     architecture-specific flags
                  #              AArch64 uses 0 for standard ABI
    ELF_HDR_SZ,   # e_ehsize:    size of this header (64 bytes)
    PHDR_SZ,      # e_phentsize: size of one program header entry (56 bytes)
    1,            # e_phnum:     number of program header entries
                  #              we only need one PT_LOAD segment
    64,           # e_shentsize: size of one section header entry
                  #              conventional to fill in even with e_shnum = 0
    0,            # e_shnum:     number of section headers (none)
    0,            # e_shstrndx:  index of section name string table
                  #              SHN_UNDEF (0) since we have no sections
)

# ── Program header (56 bytes) ────────────────────────
# Tells the kernel how to map the file into memory.
# We only need one segment: load the entire file as readable+executable.
#
# IMPORTANT: unlike 32-bit ELF, in 64-bit ELF p_flags comes SECOND,
# right after p_type — before the offset and address fields.
# This is a common source of bugs when porting 32-bit parsers.
#
# Format: < = little-endian, I = u32, Q = u64
phdr = struct.pack('<IIQQQQQQ',
    1,            # p_type:    1 = PT_LOAD — this segment gets mapped into memory
                  #            other types: 2=PT_DYNAMIC, 3=PT_INTERP, 6=PT_PHDR
    0x5,          # p_flags:   PF_R (0x4) | PF_X (0x1) = read + execute
                  #            data segments would use PF_R | PF_W (0x6) instead
                  #            ← NOTE: this field is at offset 4 in 64-bit,
                  #              but at offset 24 in 32-bit (after address fields)
    0,            # p_offset:  offset in file where segment data starts
                  #            0 = map from the very beginning of the file
                  #            (so ELF+program headers get mapped too)
    LOAD_ADDR,    # p_vaddr:   virtual address to map segment to (0x400000)
    LOAD_ADDR,    # p_paddr:   physical address — ignored on modern Linux,
                  #            conventionally same as p_vaddr
    FILE_SIZE,    # p_filesz:  how many bytes to read from the file
    FILE_SIZE,    # p_memsz:   how many bytes to reserve in memory
                  #            memsz > filesz means zero-fill the difference
                  #            (used for .bss — zero-init data that wastes file space)
    0x200000,     # p_align:   segment alignment (must be power of 2)
                  #            p_vaddr % p_align must equal p_offset % p_align
                  #            0x200000 = 2MB, matches Linux's huge page granularity
)

# ── assemble and write ────────────────────────────────
binary = ident + ehdr + phdr + code

# Sanity checks — catch struct.pack mistakes before writing
assert len(ident) + len(ehdr) == ELF_HDR_SZ, "ELF header wrong size"
assert len(phdr) == PHDR_SZ,                  "Program header wrong size"
assert len(binary) == FILE_SIZE,              "File size mismatch"

with open('exit0', 'wb') as f:
    f.write(binary)

os.chmod('exit0', os.stat('exit0').st_mode | stat.S_IEXEC)
print(f"Written {len(binary)} bytes")
print(f"Entry point: {hex(ENTRY)}")