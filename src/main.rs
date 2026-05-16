use std::env;
use std::process::Command;

fn main() {
    // read env variables that were set in build script
    let bios_path = env!("BIOS_PATH");
    println!("BIOS_PATH: {bios_path}");

    let mut cmd = Command::new("qemu-system-x86_64");
    // print serial output to the shell
    cmd.arg("-serial").arg("stdio");
    // don't display video output
    cmd.arg("-display").arg("none");
    // enable the guest to exit qemu
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-drive")
        .arg(format!("format=raw,file={bios_path}"));

    let mut child = cmd.spawn().expect("failed to start qemu-system-x86_64");
    let status = child.wait().expect("failed to wait on qemu");
    match status.code().unwrap_or(1) {
        0x10 => 0, // success
        0x11 => 1, // failure
        _ => 2,    // unknown fault
    };
}
