# Kaitai Struct

[Kaitai Struct] is a declarative YAML-based language for describing binary file formats. It generates parsers in various programming languages, making it easy to read and write complex binary formats like ELF.

The main parts of a Kaitai Struct definition are:

- **meta**: metadata about the format, such as endianness and file extension
- **seq**: the sequence of fields in the file, with their types and sizes
- **types**: definitions of custom types used in the `seq` or other types
- **instances**: computed properties that can perform calculations or lookups based on the parsed data

This directory contains three versions of a Kaitai Struct definition for ELF:

- `elf.ksy`: a complete and detailed definition covering most ELF features — taken from the Kaitai Struct repository
- `elf-min.ksy`: a minimal definition covering just the ELF header and basic references to the program and section headers. Assumes little-endian and 64-bit for simplicity.
- `elf-med.ksy`: a medium-complexity definition. Switches endianness based on the `data_encoding` byte in the ELF header, and includes more fields and types than the minimal version.

[Kaitai Struct]: https://kaitai.io/