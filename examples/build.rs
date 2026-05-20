//! Wire up the userlib-provided `user.x` linker script for every bin in
//! this crate.
//!
//! userlib's build.rs writes `user.x` to its OUT_DIR and exposes the path
//! via `DEP_USERLIB_USER_X_DIR` (because of `links = "userlib"`). We add
//! that directory to the linker search path and tell rustc to pull
//! `user.x` in for binary targets.

use std::env;

fn main() {
    let dir = env::var("DEP_USERLIB_USER_X_DIR").expect("userlib build script exposes user_x_dir");
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rustc-link-arg-bins=-Tuser.x");
    println!("cargo:rerun-if-changed=build.rs");
}
