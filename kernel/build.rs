use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let lm3s6965evb = env::var_os("CARGO_FEATURE_LM3S6965EVB").is_some();
    let lpc1768 = env::var_os("CARGO_FEATURE_LPC1768").is_some();

    let source = match (lm3s6965evb, lpc1768) {
        (true, false) => "memory/lm3s6965evb.x",
        (false, true) => "memory/lpc1768.x",
        (true, true) => panic!(
            "both `lm3s6965evb` and `lpc1768` features are enabled; pick exactly one (try \
             `cargo <cmd> --no-default-features --features lpc1768`)"
        ),
        (false, false) => panic!(
            "no board feature selected; enable exactly one of `lm3s6965evb` or `lpc1768`"
        ),
    };

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    fs::copy(source, out.join("memory.x")).expect("source memory.x exists and OUT_DIR is writable");
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-changed=build.rs");
}
