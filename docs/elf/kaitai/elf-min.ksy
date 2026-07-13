meta:
  id: elf-le-64
  endian: le   # hard-coded little-endian; ELF files could be big-endian too

# ============================================================
# ELF FILE HEADER  (always starts at byte 0)
#
# The header is 52 bytes for ELF32, 64 bytes for ELF64.
# It contains file-wide metadata and the offsets/counts needed
# to locate the program header table and section header table.
# ============================================================
seq:
  - id: magic        # must be \x7fELF
    contents: [0x7f, 0x45, 0x4c, 0x46]
  - id: elf_class    # 2 = 64-bit
    type: u1
  - id: encoding     # 1 = little-endian
    type: u1
  - id: pad
    size: 10         # elf_version, os_abi, abi_version, reserved
  - id: e_type       # 2 = exec, 3 = .so/PIE
    type: u2
  - id: e_machine    # 0x3e = x86-64
    type: u2
  - id: e_version
    type: u4
  - id: e_entry      # virtual address of _start
    type: u8
  - id: e_phoff      # file offset to program header table
    type: u8
  - id: e_shoff      # file offset to section header table
    type: u8
  - id: e_flags
    type: u4
  - id: e_ehsize     # size of this header (should be 64)
    type: u2
  - id: e_phentsize  # size of one program header (should be 56)
    type: u2
  - id: e_phnum      # number of segments
    type: u2
  - id: e_shentsize  # size of one section header (should be 64)
    type: u2
  - id: e_shnum      # number of sections
    type: u2
  - id: e_shstrndx   # which section holds the name strings
    type: u2

instances:
  segments:
    pos: e_phoff
    repeat: expr
    repeat-expr: e_phnum
    type: phdr64

  sections:
    pos: e_shoff
    repeat: expr
    repeat-expr: e_shnum
    type: shdr64

# ============================================================
# TYPES
# ============================================================
types:
  
  # ----------------------------------------------------------
  # Program header (64-bit)
  #
  # Each program header describes one *segment*: a contiguous
  # region of the file that the OS maps into the process's
  # virtual address space at load time. Common segment types:
  #   LOAD    — code or data that is actually mapped (most important)
  #   DYNAMIC — contains the dynamic linking info table
  #   INTERP  — path to the dynamic linker (e.g. /lib/ld-linux.so.2)
  #   GNU_STACK — controls whether the stack is executable
  #
  # NOTE: In ELF64 the `p_flags` field comes BEFORE the size
  # fields. This is reversed in ELF32 (see elf-med.ksy).
  # ----------------------------------------------------------
  phdr64:
    seq:
      - id: p_type    # 1=LOAD, 2=DYNAMIC, 3=INTERP…
        type: u4
      - id: p_flags   # R=4, W=2, X=1
        type: u4
      - id: p_offset  # where in the file this segment starts
        type: u8
      - id: p_vaddr   # where it maps in virtual memory
        type: u8
      - id: p_paddr   # physical addr (usually same as vaddr)
        type: u8
      - id: p_filesz  # bytes in file
        type: u8
      - id: p_memsz   # bytes in memory (>= filesz; extra is zeroed)
        type: u8
      - id: p_align
        type: u8

  # ----------------------------------------------------------
  # Section header (64-bit)
  #
  # Each section header describes one *section*: a named region
  # of the file used by the linker and debugger. Unlike segments,
  # sections can overlap with each other, and a single segment
  # often covers multiple sections (e.g. a LOAD segment contains
  # both .text and .rodata sections).
  #
  # Key sections you'll encounter:
  #   .text      — executable machine code (type: PROGBITS)
  #   .data      — initialised read-write data (type: PROGBITS)
  #   .rodata    — read-only constants, string literals
  #   .bss       — uninitialised data (type: NOBITS — no file bytes)
  #   .symtab    — full symbol table for static linking/debug
  #   .dynsym    — minimal symbol table for dynamic linking
  #   .shstrtab  — null-terminated strings giving section names
  #   .dynamic   — dynamic linking info (DT_NEEDED entries, etc.)
  #   .rela.*    — relocation records with addend
  # ----------------------------------------------------------
  shdr64:
    seq:
      - id: sh_name    # offset into .shstrtab, not the string itself
        type: u4
      - id: sh_type    # 1=PROGBITS, 3=STRTAB, 8=NOBITS(.bss)…
        type: u4
      - id: sh_flags   # section flags
        type: u8
      - id: sh_addr
        type: u8
      - id: sh_offset  # file offset to section data
        type: u8
      - id: sh_size
        type: u8
      - id: sh_link
        type: u4
      - id: sh_info
        type: u4
      - id: sh_addralign
        type: u8
      - id: sh_entsize
        type: u8