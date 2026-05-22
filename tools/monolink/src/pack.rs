//! Pack a userlib-linked ELF into a monoexec `.bin`.
//!
//! [`pack_into`] is the core: ELF bytes → header + body bytes via any
//! [`Write`] sink. [`pack_paths`] is a thin filesystem convenience the
//! CLI uses.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use monolink_proto::ExecHeader;
use object::{Object, ObjectSection, ObjectSymbol};

/// What ended up in the packed `.bin`.
///
/// Returned so callers can log byte counts and entry resolution without
/// having to re-parse what they just wrote.
#[derive(Debug, Clone, Copy)]
pub struct PackSummary {
    pub text_size: u32,
    pub data_size: u32,
    pub bss_size: u32,
    pub entry_offset: u32,
    pub timeout_ms: u32,
    /// Total bytes written to the output sink (header + text + data).
    pub total_bytes: usize,
}

/// Read `elf` bytes, write the monoexec header + body to `out`.
pub fn pack_into<W: Write>(elf: &[u8], out: &mut W, timeout_ms: u32) -> Result<PackSummary> {
    let obj = object::File::parse(elf).context("parsing ELF")?;

    // .text holds code + rodata (merged by user.x). .data holds the init
    // image. .bss is NOLOAD — only its size matters.
    let text = obj
        .section_by_name(".text")
        .ok_or_else(|| anyhow!("ELF missing .text"))?;
    let data = obj.section_by_name(".data");
    let bss = obj.section_by_name(".bss");

    let text_bytes = text.data().context("reading .text contents")?;
    let text_size = u32::try_from(text_bytes.len()).context(".text larger than 4 GiB")?;

    let (data_bytes, data_size): (&[u8], u32) = match data {
        Some(ref s) => {
            let b = s.data().context("reading .data contents")?;
            let n = u32::try_from(b.len()).context(".data larger than 4 GiB")?;
            (b, n)
        }
        None => (&[], 0),
    };

    let bss_size = match bss {
        Some(ref s) => u32::try_from(s.size()).context(".bss larger than 4 GiB")?,
        None => 0,
    };

    // entry_offset is the byte offset of `_start` from the user_code base.
    // user.x places .text at user_code base and KEEPs `.text._start` first,
    // so this should be zero — but compute it so a future linker-script
    // change doesn't silently break entry resolution.
    //
    // Thumb-function symbols carry bit 0 set per the ARM ELF ABI; mask it
    // off before subtracting. The kernel OR's the bit back on at launch.
    let start_sym = obj
        .symbols()
        .find(|s| s.name().ok() == Some("_start"))
        .ok_or_else(|| anyhow!("ELF missing `_start` symbol"))?;
    let entry_addr = start_sym.address() & !1;
    let text_base = text.address();
    if entry_addr < text_base {
        bail!(
            "`_start` at {:#x} lies before .text base {:#x}",
            entry_addr,
            text_base,
        );
    }
    let entry_offset = u32::try_from(entry_addr - text_base).with_context(|| {
        format!(
            "entry_offset {:#x} doesn't fit in u32",
            entry_addr - text_base
        )
    })?;

    let header = ExecHeader {
        magic: ExecHeader::MAGIC,
        version: ExecHeader::VERSION,
        flags: 0,
        entry_offset,
        text_size,
        data_size,
        bss_size,
        timeout_ms,
        reserved: 0,
    };
    let mut header_bytes = [0u8; ExecHeader::SIZE];
    header.write_le(&mut header_bytes);

    out.write_all(&header_bytes).context("writing header")?;
    out.write_all(text_bytes).context("writing .text")?;
    out.write_all(data_bytes).context("writing .data")?;

    Ok(PackSummary {
        text_size,
        data_size,
        bss_size,
        entry_offset,
        timeout_ms,
        total_bytes: ExecHeader::SIZE + text_bytes.len() + data_bytes.len(),
    })
}

/// Path-based convenience: read `elf_path`, write `out_path`.
pub fn pack_paths(elf_path: &Path, out_path: &Path, timeout_ms: u32) -> Result<PackSummary> {
    let elf_bytes =
        std::fs::read(elf_path).with_context(|| format!("reading {}", elf_path.display()))?;
    let mut out =
        File::create(out_path).with_context(|| format!("creating {}", out_path.display()))?;
    pack_into(&elf_bytes, &mut out, timeout_ms)
}
