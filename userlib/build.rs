//! Materializes `user.x` for the selected board feature.
//!
//! Reads `user.x.in`, substitutes `@USER_CODE_BASE@` / `@USER_DATA_BASE@` /
//! region sizes, writes the result to `OUT_DIR/user.x`, and exposes the
//! containing directory so consumer crates can pass `-Tuser.x` via their
//! own build script.
//!
//! Layout numbers MUST track `kernel/src/user.rs`. The kernel rejects loads
//! that don't fit, but a silent address mismatch would only show up as a
//! fault inside the user program — easier to spot if both sides read the
//! same constants.

use std::env;
use std::fs;
use std::path::PathBuf;

struct Layout {
    /// First instruction the kernel branches into (= user_text base + trampoline len).
    user_code_base: u32,
    /// Bytes available for `.text` + `.rodata` (user_text region minus trampoline).
    user_text_capacity: u32,
    /// Base of the user_data MPU region.
    user_data_base: u32,
    /// Total size of user_data (covers `.data` init image + `.bss`).
    user_data_size: u32,
}

// Mirrored from kernel/src/user.rs.
const TRAMPOLINE_LEN: u32 = 12;
const USER_TEXT_SIZE: u32 = 0x2000;
const USER_DATA_SIZE: u32 = 0x2000;

fn layout_for(board: &str) -> Layout {
    let (text_base, data_base) = match board {
        // lpc1768 user regions live in AHB SRAM banks 0+1 so the kernel
        // keeps the full 32 KiB local SRAM at 0x1000_0000 for itself.
        "lpc1768" => (0x2007_C000_u32, 0x2007_E000_u32),
        "lm3s6965evb" => (0x2000_8000_u32, 0x2000_A000_u32),
        other => panic!("unknown board feature `{other}`"),
    };
    Layout {
        user_code_base: text_base + TRAMPOLINE_LEN,
        user_text_capacity: USER_TEXT_SIZE - TRAMPOLINE_LEN,
        user_data_base: data_base,
        user_data_size: USER_DATA_SIZE,
    }
}

fn main() {
    let lpc1768 = env::var_os("CARGO_FEATURE_LPC1768").is_some();
    let lm3s = env::var_os("CARGO_FEATURE_LM3S6965EVB").is_some();
    let board = match (lpc1768, lm3s) {
        (true, false) => "lpc1768",
        (false, true) => "lm3s6965evb",
        (true, true) => {
            panic!("both `lm3s6965evb` and `lpc1768` features are enabled; pick exactly one")
        }
        (false, false) => {
            panic!("no board feature selected; enable exactly one of `lm3s6965evb` or `lpc1768`")
        }
    };
    let layout = layout_for(board);

    let template = fs::read_to_string("user.x.in").expect("user.x.in exists");
    let rendered = template
        .replace(
            "@USER_CODE_BASE@",
            &format!("0x{:08X}", layout.user_code_base),
        )
        .replace(
            "@USER_TEXT_CAPACITY@",
            &format!("0x{:X}", layout.user_text_capacity),
        )
        .replace(
            "@USER_DATA_BASE@",
            &format!("0x{:08X}", layout.user_data_base),
        )
        .replace(
            "@USER_DATA_SIZE@",
            &format!("0x{:X}", layout.user_data_size),
        );

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    let user_x = out.join("user.x");
    fs::write(&user_x, rendered).expect("write user.x");

    // `links = "userlib"` in Cargo.toml turns this into DEP_USERLIB_USER_X_DIR
    // for dependents. Their build.rs reads it to add `-L OUT_DIR -Tuser.x`.
    println!("cargo:user_x_dir={}", out.display());

    println!("cargo:rerun-if-changed=user.x.in");
    println!("cargo:rerun-if-changed=build.rs");
}
