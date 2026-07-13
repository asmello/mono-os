# Extensible and Linkable Format (ELF)

An **ELF** (Executable and Linkable Format) file is the standard binary format used on Linux and most Unix-like systems.

First developed for Unix System V Release 4 (SVR4) in the late 1980s, ELF was designed to be flexible and extensible across different architectures and use cases. Today it is used by Linux, BSDs, Solaris, Android, PlayStation, microcontrollers, and many other platforms.

It is not only used for executables, but also for shared libraries (`.so` files), object files (`.o`), and even core dumps. The format is designed to support both static and dynamic linking, and can contain rich metadata for debugging and linking.

## Structure

An ELF file is composed of:

- **ELF Header**: a fixed-size structure at the start of the file describing the file type, target architecture, entry point address, and offsets to the program and section header tables.
- **Program Header Table**: describes *segments*, which are used by the OS loader at runtime to map the file into memory. Each entry specifies a type (e.g. `PT_LOAD`, `PT_DYNAMIC`), virtual address, file offset, and permissions (read/write/execute).
- **Section Header Table**: describes *sections*, which are used by the linker. Sections like `.text` (code), `.data` (initialised data), `.bss` (zero-initialised data), `.rodata` (read-only data), `.symtab` (symbol table), and `.debug_*` (DWARF debug info) live here.
- **The actual content**: the raw bytes of code, data, relocations, etc. that the headers point into.

```text
    ┌─────────────────────┐
    │     ELF Header      │ ← Metadata: arch, entry point, offsets to tables
    ├─────────────────────┤
    │   Program Headers   │───┐ ← Points to segments (tells the OS loader how
    ├─────────────────────┤   │   to map the file into memory)
┌──▶│        .text        │⎫  │
│   ├─────────────────────┤⎬ ◀┤
│   │        .data        │⎪  │
│   ├─────────────────────┤⎭  │
│┌─▶│        .bss         │⎫  │
││  ├─────────────────────┤⎬ ◀┘
││  │         ...         │⎭
││  ├─────────────────────┤
└┴──│   Section Headers   │ ← Tells the linker about sections
    └─────────────────────┘
```

The distinction between segments and sections is important. Sections are a linker concept (fine-grained, used at link time), while segments are a loader concept (coarser, used at runtime). A single segment often spans multiple sections.

Segments and sections are two parallel views of the same data. The program headers describe how to load the file into memory, while the section headers describe how to link it together. This duality is a key design feature of ELF, allowing it to serve both as a linkable object format and an executable format.

## File Types

The `e_type` field in the header identifies what kind of ELF it is:

| Type      | Meaning                                                               |
|-----------|-----------------------------------------------------------------------|
| `ET_REL`  | Relocatable object file (`.o`) — output of a compiler, not yet linked |
| `ET_EXEC` | Statically linked executable                                          |
| `ET_DYN`  | Position-independent executable or shared library (`.so`)             |
| `ET_CORE` | Core dump                                                             |

## How it works at runtime

When you execute an ELF binary, the kernel reads the program headers and maps each `PT_LOAD` segment into the process's virtual address space. If the file is dynamically linked, the `PT_INTERP` segment names the dynamic linker (usually `/lib64/ld-linux-x86-64.so.2`), which the kernel invokes to resolve shared library dependencies before handing control to the entry point.

## Useful tools

- `readelf -a <file>`: inspect all headers, sections, and symbols
- `objdump -d <file>`: disassemble
- `nm <file>`: list symbols
- `file <file>`: quick identification
- `patchelf`: modify interpreter or RPATH in an existing ELF
