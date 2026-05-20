//! Simplest possible user program: exit with code 42.
//!
//! End-to-end smoke test for the userlib → user.x → monolink-pack → kernel
//! path. The kernel should report `status=OK, exit_code=0x2A`.

#![no_std]
#![no_main]

fn main() {
    userlib::exit(42);
}

userlib::entry!(main);
