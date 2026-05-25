//! ARMv7-M Memory Protection Unit bring-up for the user sandbox.
//!
//! The MPU is left disabled at boot — cortex-m-rt doesn't touch it, so the
//! kernel runs with the architectural default memory map and unrestricted
//! access. [`enable_user_regions`] programs three regions just before
//! handing control to an unprivileged user program, and [`disable`] turns
//! the unit back off after the user terminates.
//!
//! Region assignment (ARMv7-M ARM B3.5):
//!   0  user .text   — RX, kernel may write to load programs (AP = priv RW / unpriv RO)
//!   1  user .data   — RW from both sides (AP = priv RW / unpriv RW), XN
//!   2  user .stack  — same as data
//!
//! `PRIVDEFENA=1` keeps the privileged kernel's view unrestricted everywhere
//! else, so we only have to enumerate the three user-visible windows.
//! Memory attributes are `Normal, Non-shareable, Write-back, Write-allocate`
//! (TEX=0, S=0, C=1, B=1) — the standard SRAM encoding.

use core::ptr;

use cortex_m::asm::{dsb, isb};

// Register addresses (ARMv7-M ARM B3.5.1).
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;

const CTRL_ENABLE: u32 = 1 << 0;
const CTRL_PRIVDEFENA: u32 = 1 << 2;

// Access-permission encodings for the AP field (ARMv7-M ARM B3.5.5 Table B3-15).
const AP_PRIV_RW_UNPRIV_RO: u32 = 0b010;
const AP_PRIV_RW_UNPRIV_RW: u32 = 0b011;

// TEX/S/C/B for Normal, Non-shareable, Write-back, Write-allocate (B3.5.6).
// Layout: TEX=0b000 [21:19], S=0 [18], C=1 [17], B=1 [16].
const ATTRS_NORMAL_WB: u32 = (1 << 17) | (1 << 16);

/// Region 0 carries the user `.text` and is fixed at 8 KiB.
const USER_TEXT_SIZE_LOG2: u32 = 13;
/// Region 1 carries `.data` + `.bss` and is fixed at 8 KiB.
const USER_DATA_SIZE_LOG2: u32 = 13;
/// Region 2 carries the user stack and is fixed at 16 KiB.
const USER_STACK_SIZE_LOG2: u32 = 14;

/// Program the three user regions and turn the MPU on.
///
/// All three bases must be naturally aligned (power-of-two alignment equal
/// to the region size). Caller is responsible for that — the layout
/// constants in [`crate::user`] satisfy it by construction.
pub fn enable_user_regions(text_base: u32, data_base: u32, stack_base: u32) {
    write_region(
        0,
        text_base,
        USER_TEXT_SIZE_LOG2,
        AP_PRIV_RW_UNPRIV_RO,
        false,
    );
    write_region(
        1,
        data_base,
        USER_DATA_SIZE_LOG2,
        AP_PRIV_RW_UNPRIV_RW,
        true,
    );
    write_region(
        2,
        stack_base,
        USER_STACK_SIZE_LOG2,
        AP_PRIV_RW_UNPRIV_RW,
        true,
    );
    unsafe {
        dsb();
        ptr::write_volatile(MPU_CTRL, CTRL_ENABLE | CTRL_PRIVDEFENA);
        dsb();
        isb();
    }
}

pub fn disable() {
    unsafe {
        dsb();
        ptr::write_volatile(MPU_CTRL, 0);
        dsb();
        isb();
    }
}

fn write_region(idx: u32, base: u32, size_log2: u32, ap: u32, xn: bool) {
    // RASR.SIZE field = log2(bytes) - 1.
    let size_field = (size_log2 - 1) & 0x1F;
    let rbar = (base & !0x1F) | (1 << 4) | (idx & 0xF);
    let rasr = 1 // ENABLE
        | (size_field << 1)
        | ATTRS_NORMAL_WB
        | (ap << 24)
        | ((xn as u32) << 28);
    unsafe {
        ptr::write_volatile(MPU_RNR, idx);
        // Disable the region before reprogramming the base, otherwise a stale
        // SIZE could combine with the new base to fault on the write itself.
        ptr::write_volatile(MPU_RASR, 0);
        ptr::write_volatile(MPU_RBAR, rbar);
        ptr::write_volatile(MPU_RASR, rasr);
    }
}
