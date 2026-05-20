//! Write to an address outside any user MPU region.
//!
//! Address 0 holds the vector table on Cortex-M — well outside the
//! user_text / user_data / user_stack regions on every supported board.
//! With PRIVDEFENA + unprivileged thread mode the MPU denies the write
//! and the kernel should report `status=MEMFAULT`.

#![no_std]
#![no_main]

fn main() {
    unsafe {
        // `0` is the vector table on Cortex-M and outside every user MPU
        // region; the store faults before the value is even latched.
        let p: *mut u32 = core::ptr::without_provenance_mut(0);
        core::ptr::write_volatile(p, 0xDEAD_BEEF);
    }
    userlib::exit(0);
}

userlib::entry!(main);
