//! `monolink` CLI: ship a user-program `.bin` to the device, print the RESULT.
//!
//! Thin wrapper over the [`monolink`] library. Out-of-process callers
//! (the `exact-runner` agent, the `exact-api` build worker) should depend
//! on the library directly rather than shelling out to this binary.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser as ClapParser, Subcommand};
use monolink::proto::{
    CaseResultFrame, ResultFrame, STATUS_BUSFAULT, STATUS_LOAD_ERROR, STATUS_MEMFAULT, STATUS_OK,
    STATUS_TIMEOUT, STATUS_USAGEFAULT,
};
use monolink::{Loader, PackSummary, demo, open_serial, pack_paths};

#[derive(ClapParser)]
#[command(about = "Host loader for the monolink wire protocol")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Send a .bin to the device, print the RESULT frame.
    Upload {
        /// Serial device, e.g. /dev/cu.usbmodem* or a QEMU `-serial pty`
        port: String,
        /// Path to a monoexec .bin (header + body)
        bin: PathBuf,
        #[arg(long, default_value_t = 115_200)]
        baud: u32,
        /// Cases file: sequence of `[u16 LE length][bytes]` records.
        ///
        /// Each record drives one program invocation; the kernel
        /// relaunches the program with fresh `.data`/`.bss` per case.
        /// If omitted, the host runs exactly one case with empty
        /// input (compatible with single-shot programs).
        #[arg(long)]
        cases: Option<PathBuf>,
    },
    /// Turn a userlib-linked ELF into a monoexec .bin.
    Pack {
        /// Path to the ELF produced by `cargo build` (release recommended)
        elf: PathBuf,
        /// Output .bin path
        out: PathBuf,
        /// Kernel-enforced deadline in milliseconds
        #[arg(long, default_value_t = 100)]
        timeout_ms: u32,
    },
    /// Write a tiny exit(42) .bin for sanity tests.
    Demo {
        /// Output path
        out: PathBuf,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Upload {
            port,
            bin,
            baud,
            cases,
        } => upload(&port, &bin, baud, cases.as_deref()),
        Cmd::Pack {
            elf,
            out,
            timeout_ms,
        } => {
            let s = pack_paths(&elf, &out, timeout_ms)?;
            print_pack(&out, &s);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Demo { out } => {
            let bytes = demo::build_exit42();
            std::fs::write(&out, &bytes).with_context(|| format!("creating {}", out.display()))?;
            println!(
                "wrote {} ({} bytes): runs `movs r0,#42; svc #0`",
                out.display(),
                bytes.len(),
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn upload(port_path: &str, bin: &Path, baud: u32, cases_path: Option<&Path>) -> Result<ExitCode> {
    let raw = std::fs::read(bin).with_context(|| format!("reading {}", bin.display()))?;

    let cases: Vec<Vec<u8>> = match cases_path {
        Some(p) => read_cases_file(p)?,
        // No cases file: one synthetic empty case so single-shot programs
        // get exactly one program invocation.
        None => vec![Vec::new()],
    };

    let transport = open_serial(port_path, baud)?;
    let mut loader = Loader::new(transport);
    let report = loader.upload_and_run(&raw, &cases, |idx, cr, output| {
        print_case_result(idx, cr, output);
    })?;
    print_result(&report.overall);
    Ok(if report.overall.status == STATUS_OK {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

/// Cases file format: a sequence of `[u16 LE length][bytes]` records.
fn read_cases_file(path: &Path) -> Result<Vec<Vec<u8>>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut cases = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if cursor + 2 > bytes.len() {
            bail!(
                "{}: truncated length prefix at offset {cursor}",
                path.display()
            );
        }
        let len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + len > bytes.len() {
            bail!(
                "{}: case at offset {} claims {} bytes, only {} remain",
                path.display(),
                cursor - 2,
                len,
                bytes.len() - cursor,
            );
        }
        cases.push(bytes[cursor..cursor + len].to_vec());
        cursor += len;
    }
    Ok(cases)
}

fn status_name(status: u8) -> &'static str {
    match status {
        STATUS_OK => "OK",
        STATUS_TIMEOUT => "TIMEOUT",
        STATUS_MEMFAULT => "MEMFAULT",
        STATUS_BUSFAULT => "BUSFAULT",
        STATUS_USAGEFAULT => "USAGEFAULT",
        STATUS_LOAD_ERROR => "LOAD_ERROR",
        _ => "?",
    }
}

fn print_case_result(idx: usize, cr: &CaseResultFrame, output: &[u8]) {
    let output_repr = if output.is_empty() {
        "(none)".to_string()
    } else {
        let hex: String = output.iter().map(|b| format!("{b:02x}")).collect();
        format!("0x{hex}")
    };
    println!(
        "case[{idx}] status={} exit={:#010x} cycles={} output={output_repr}",
        status_name(cr.status),
        cr.exit_code,
        cr.cycles,
    );
}

fn print_result(r: &ResultFrame) {
    println!(
        "RESULT status={} cclk_hz={}",
        status_name(r.status),
        r.cclk_hz
    );
}

fn print_pack(out_path: &Path, s: &PackSummary) {
    println!(
        "wrote {} ({} bytes): text={} data={} bss={} entry={:#x} timeout={}ms",
        out_path.display(),
        s.total_bytes,
        s.text_size,
        s.data_size,
        s.bss_size,
        s.entry_offset,
        s.timeout_ms,
    );
}
