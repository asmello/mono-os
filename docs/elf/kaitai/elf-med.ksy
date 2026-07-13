meta:
  id: elf
  title: ELF Executable and Linkable Format
  file-extension:
    - elf
    - so
    - o

  # ELF files can be either little-endian (x86, ARM LE, RISC-V)
  # or big-endian (SPARC, old MIPS, etc.). We cannot know which
  # until we have read the `data_encoding` byte inside e_ident,
  # so we declare a switch here. Kaitai will re-read all
  # subsequent multi-byte integers with the correct byte order.
  endian:
    switch-on: _root.e_ident.data_encoding
    cases:
      data_encoding::lsb: le
      data_encoding::msb: be

# ============================================================
# ELF FILE HEADER  (always starts at byte 0)
#
# The header is 52 bytes for ELF32, 64 bytes for ELF64.
# It contains file-wide metadata and the offsets/counts needed
# to locate the program header table and section header table.
# ============================================================
seq:
  # The first 16 bytes are the "ident" array — parsed as a
  # sub-type below so each field has a name.
  - id: e_ident
    type: ident

  # Type of ELF object: relocatable, executable, shared, core…
  - id: e_type
    type: u2
    enum: obj_type

  # Target ISA, e.g. x86_64 (0x3e), AArch64 (0xb7), RISC-V (0xf3).
  - id: e_machine
    type: u2
    enum: machine

  # ELF format version — always 1 in practice (EV_CURRENT).
  - id: e_version
    type: u4

  # Virtual address of the program entry point (_start).
  # 0 for objects that have no entry point (e.g. .so files).
  # Width varies: 4 bytes for ELF32, 8 bytes for ELF64.
  - id: e_entry
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: u4
        elf_class::elf64: u8

  # File offset (in bytes from the start of the file) to the
  # first entry of the program header table.
  # 0 if there is no program header table (e.g. a .o file).
  - id: e_phoff
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: u4
        elf_class::elf64: u8

  # File offset to the section header table.
  # 0 if there is no section header table.
  - id: e_shoff
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: u4
        elf_class::elf64: u8

  # Processor-specific flags (e.g. ARM ABI version, RISC-V ISA
  # extensions). Usually 0 on x86/x86-64.
  - id: e_flags
    type: u4

  # Total size of this ELF header in bytes (52 for ELF32,
  # 64 for ELF64). Useful as a sanity check.
  - id: e_ehsize
    type: u2

  # Size in bytes of ONE program header entry. Multiplying by
  # e_phnum gives the total size of the program header table.
  - id: e_phentsize
    type: u2

  # Number of program header entries (segments).
  - id: e_phnum
    type: u2

  # Size in bytes of ONE section header entry.
  - id: e_shentsize
    type: u2

  # Number of section header entries.
  - id: e_shnum
    type: u2

  # Index (0-based) into the section header table of the entry
  # that describes the section name string table (.shstrtab).
  # We use this below to resolve section names from offsets.
  - id: e_shstrndx
    type: u2

# ============================================================
# TYPES
# ============================================================
types:

  # ----------------------------------------------------------
  # e_ident — the 16-byte identification array at offset 0
  #
  # These bytes are intentionally endian-neutral so a loader
  # can inspect them before deciding byte-order or word size.
  # ----------------------------------------------------------
  ident:
    seq:
      # Magic number: 0x7f followed by ASCII "ELF".
      # Kaitai will raise a parse error if these 4 bytes do
      # not match exactly — a fast file-type sanity check.
      - id: magic
        contents: [0x7f, 0x45, 0x4c, 0x46]   # \x7fELF

      # Whether this is a 32-bit or 64-bit ELF.
      # Controls word widths for addresses and offsets
      # throughout the rest of the file.
      - id: elf_class
        type: u1
        enum: elf_class

      # Byte order to use when reading multi-byte fields:
      # 1 = LSB (little-endian), 2 = MSB (big-endian).
      # Referenced by the top-level `endian` switch above.
      - id: data_encoding
        type: u1
        enum: data_encoding

      # ELF spec version used when producing this file.
      # Should always be 1; higher values are reserved.
      - id: elf_version
        type: u1

      # Identifies the OS/ABI conventions the binary targets.
      # Most Linux binaries use SYSTEM_V (0x00) even though
      # they run on Linux — the ABI is effectively the same.
      - id: os_abi
        type: u1
        enum: os_abi

      # ABI-specific version number; meaning depends on os_abi.
      # Typically 0.
      - id: abi_version
        type: u1

      # 7 bytes of padding to reach exactly 16 bytes total.
      # The ELF spec reserves these for future use; should be 0.
      - id: pad
        size: 7

  # ----------------------------------------------------------
  # Program header (32-bit)
  #
  # Each program header describes one *segment*: a contiguous
  # region of the file that the OS maps into the process's
  # virtual address space at load time. Common segment types:
  #   LOAD    — code or data that is actually mapped (most important)
  #   DYNAMIC — contains the dynamic linking info table
  #   INTERP  — path to the dynamic linker (e.g. /lib/ld-linux.so.2)
  #   GNU_STACK — controls whether the stack is executable
  #
  # NOTE: In ELF32 the `p_flags` field comes AFTER the size
  # fields. This is reversed in ELF64 (see phdr64 below).
  # ----------------------------------------------------------
  phdr32:
    seq:
      # Segment type — determines how the OS should treat it.
      - id: p_type
        type: u4
        enum: ph_type

      # Byte offset from the start of the file to this segment.
      - id: p_offset
        type: u4

      # Virtual address at which the segment should be loaded
      # into the process's address space.
      - id: p_vaddr
        type: u4

      # Physical address — relevant for bare-metal targets;
      # usually identical to p_vaddr (or ignored) on Linux.
      - id: p_paddr
        type: u4

      # Number of bytes the segment occupies in the file.
      # May be 0 for segments like GNU_STACK that carry no data.
      - id: p_filesz
        type: u4

      # Number of bytes the segment should occupy in memory.
      # p_memsz >= p_filesz; the extra bytes (e.g. BSS) are
      # zero-filled by the loader.
      - id: p_memsz
        type: u4

      # Permission flags: bit 0 = X (execute), 1 = W (write),
      # 2 = R (read). A typical LOAD segment for code is R+X (5).
      - id: p_flags
        type: u4

      # Required alignment for this segment in memory and in
      # the file. Must be a power of two; 0 or 1 means no
      # alignment requirement.
      - id: p_align
        type: u4

  # ----------------------------------------------------------
  # Program header (64-bit)
  #
  # Identical semantics to phdr32, but:
  #   - addresses and offsets are 8 bytes wide
  #   - p_flags is moved to BEFORE p_offset (right after p_type)
  #     This was done to keep the 8-byte fields naturally aligned
  #     without needing padding between p_type and p_offset.
  # ----------------------------------------------------------
  phdr64:
    seq:
      - id: p_type
        type: u4
        enum: ph_type

      # *** FLAGS HERE (unlike phdr32) — alignment optimisation ***
      - id: p_flags
        type: u4

      - id: p_offset
        type: u8
      - id: p_vaddr
        type: u8
      - id: p_paddr
        type: u8
      - id: p_filesz
        type: u8
      - id: p_memsz
        type: u8
      - id: p_align
        type: u8

  # ----------------------------------------------------------
  # Section header (32-bit)
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
  shdr32:
    seq:
      # Byte offset of this section's name within .shstrtab.
      # The actual string is resolved in the `name` instance below.
      - id: sh_name
        type: u4

      # Section kind — determines how the linker treats the data.
      - id: sh_type
        type: u4
        enum: sh_type

      # Bit-field of section attributes, e.g.:
      #   SHF_WRITE     (0x1) — writable at run time
      #   SHF_ALLOC     (0x2) — occupies memory during execution
      #   SHF_EXECINSTR (0x4) — contains executable instructions
      - id: sh_flags
        type: u4

      # Virtual address of the section if it is loaded into
      # memory (SHF_ALLOC set); 0 otherwise.
      - id: sh_addr
        type: u4

      # Byte offset from the start of the file to this section's
      # data. For NOBITS sections (.bss) this is conceptual —
      # no bytes are stored.
      - id: sh_offset
        type: u4

      # Size of the section's data in bytes (in the file).
      # For NOBITS sections this is the size in memory,
      # not the number of bytes actually present in the file.
      - id: sh_size
        type: u4

      # General-purpose link field; interpretation depends on
      # sh_type. For symbol table sections this is the index of
      # the associated string table section.
      - id: sh_link
        type: u4

      # General-purpose info field; also type-dependent. For
      # symbol tables it is the index of the first global symbol.
      - id: sh_info
        type: u4

      # Required alignment of the section (power of two).
      # 0 or 1 means no constraint.
      - id: sh_addralign
        type: u4

      # For sections that contain fixed-size records (e.g.
      # symbol tables, relocation tables), this is the size in
      # bytes of each record. 0 for variable-size sections.
      - id: sh_entsize
        type: u4

    instances:
      # Lazily resolve the section name string:
      # seek to (start of .shstrtab data) + sh_name offset,
      # then read a null-terminated ASCII string.
      # `section_names` is the shstrtab section header, resolved
      # in the top-level `instances` block below.
      name:
        io: _root._io
        pos: >-
          _root.section_names.sh_offset +
          sh_name
        type: strz
        encoding: ASCII

  # ----------------------------------------------------------
  # Section header (64-bit)
  #
  # Same structure and semantics as shdr32; sh_flags, sh_addr,
  # sh_offset, sh_size and sh_addralign grow to 8 bytes.
  # sh_name, sh_type, sh_link, sh_info remain 4 bytes.
  # ----------------------------------------------------------
  shdr64:
    seq:
      - id: sh_name
        type: u4
      - id: sh_type
        type: u4
        enum: sh_type
      - id: sh_flags
        type: u8       # wider than ELF32
      - id: sh_addr
        type: u8
      - id: sh_offset
        type: u8
      - id: sh_size
        type: u8
      - id: sh_link
        type: u4       # still 4 bytes (section index fits in u4)
      - id: sh_info
        type: u4
      - id: sh_addralign
        type: u8
      - id: sh_entsize
        type: u8

    instances:
      # Same name-resolution logic as shdr32.
      name:
        io: _root._io
        pos: >-
          _root.section_names.sh_offset +
          sh_name
        type: strz
        encoding: ASCII

# ============================================================
# TOP-LEVEL INSTANCES  (lazily evaluated — parsed on first use)
# ============================================================
instances:

  # Parse the entire program header table as an array.
  # Kaitai seeks to `e_phoff` and reads `e_phnum` entries, each
  # of the appropriate type for the word size.
  program_headers:
    pos: e_phoff
    repeat: expr
    repeat-expr: e_phnum
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: phdr32
        elf_class::elf64: phdr64

  # Parse the entire section header table as an array.
  # Same pattern: seek to e_shoff and read e_shnum entries.
  section_headers:
    pos: e_shoff
    repeat: expr
    repeat-expr: e_shnum
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: shdr32
        elf_class::elf64: shdr64

  # Parse ONLY the .shstrtab section header (not all headers).
  #
  # The byte offset of a section header entry is:
  #   e_shoff  +  (index × e_shentsize)
  # e_shstrndx gives us the index of the string-table section,
  # so we can jump straight to it without loading the whole table.
  #
  # This instance is referenced by the `name` instance inside
  # both shdr32 and shdr64 — it provides sh_offset so that a
  # section can look up its own name without a circular dependency.
  section_names:
    pos: e_shoff + e_shstrndx * e_shentsize
    type:
      switch-on: _root.e_ident.elf_class
      cases:
        elf_class::elf32: shdr32
        elf_class::elf64: shdr64

# ============================================================
# ENUMERATIONS
# ============================================================
enums:

  # ELF class — controls address/offset widths throughout.
  elf_class:
    0: none
    1: elf32
    2: elf64

  # Byte ordering for multi-byte integers.
  data_encoding:
    0: none
    1: lsb    # little-endian (Intel, ARM LE, RISC-V)
    2: msb    # big-endian   (SPARC, old PowerPC, old MIPS)

  # Target OS/ABI. Most Linux binaries use SYSTEM_V (0x00).
  os_abi:
    0x00: system_v
    0x03: linux
    0x09: freebsd
    0x0c: openbsd

  # Overall file type.
  obj_type:
    0: no_file_type
    1: rel    # relocatable (.o) — not yet linked
    2: exec   # statically linked executable
    3: dyn    # shared object (.so) OR position-independent executable (PIE)
    4: core   # core dump

  # Target instruction set architecture.
  machine:
    0x03: x86
    0x28: arm
    0x3e: x86_64
    0xb7: aarch64
    0xf3: riscv

  # Segment type (used in program headers).
  ph_type:
    0: null_type              # ignored entry
    1: load                   # mapped into memory by the OS loader
    2: dynamic                # contains the .dynamic section (DT_* tags)
    3: interp                 # null-terminated path to the dynamic linker
    4: note                   # auxiliary OS/toolchain info
    6: phdr                   # the program header table itself (self-referential)
    7: tls                    # thread-local storage template (.tdata / .tbss)
    0x6474e550: gnu_eh_frame  # GCC exception-handling frame info (.eh_frame_hdr)
    0x6474e551: gnu_stack     # stack permissions; W flag set → exec stack (bad)
    0x6474e552: gnu_relro     # region to make read-only after dynamic linking

  # Section type (used in section headers).
  sh_type:
    0:  null_type   # inactive / placeholder
    1:  progbits    # code or data with explicit content (.text, .data, .rodata)
    2:  symtab      # full symbol table (may be stripped in release builds)
    3:  strtab      # null-terminated string table (.strtab, .shstrtab, .dynstr)
    4:  rela        # relocation records WITH explicit addend (common on x86-64)
    5:  hash        # symbol hash table (for O(1) dynamic lookup)
    6:  dynamic     # dynamic linking info (.dynamic section)
    7:  note        # build-id, ABI tags, etc. (.note.* sections)
    8:  nobits      # occupies memory but no file bytes (.bss — zero-initialised)
    9:  rel         # relocation records WITHOUT addend (common on x86 / ARM 32)
    11: dynsym      # dynamic symbol table (subset of symtab, never stripped)
    14: init_array  # array of function pointers called before main()
    15: fini_array  # array of function pointers called after main() / at exit
