use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let (image_path, test_mode) = match env::args_os().nth(1) {
        Some(elf) => {
            let elf = PathBuf::from(elf);
            let img = elf.with_extension("img");
            bootloader::BiosBoot::new(&elf)
                .create_disk_image(&img)
                .expect("kernel ELF is well-formed and target dir is writable");
            (img, true)
        }
        None => (PathBuf::from(env!("BIOS_PATH")), false),
    };

    let status = Command::new("qemu-system-x86_64")
        .arg("-serial")
        .arg("stdio")
        .arg("-display")
        .arg("none")
        .arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04")
        .arg("-drive")
        .arg(format!("format=raw,file={}", image_path.display()))
        .status()
        .expect("qemu-system-x86_64 installed and on PATH");

    if !test_mode {
        return ExitCode::SUCCESS;
    }

    // isa-debug-exit reports (kernel_code << 1) | 1.
    // QemuExitCode::Success = 0x10 -> 33; Failed = 0x11 -> 35.
    match status.code() {
        Some(33) => ExitCode::SUCCESS,
        Some(code) => {
            eprintln!("test failed: qemu exited with code {code}");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("test failed: qemu terminated by signal");
            ExitCode::FAILURE
        }
    }
}
