//! Run one untrusted program under MPU and a hardware deadline.
//!
//! Drops to unprivileged thread mode on PSP, terminates on `exit` syscall,
//! fault, or deadline, and reports status + cycle count back to the caller.
//!
//! ## Privilege transition
//!
//! There is no atomic "drop-privilege-and-branch" instruction in ARMv7-M
//! thread mode. After `MSR CONTROL, ...; ISB`, the very next fetch happens
//! at the new privilege level, so the *instruction following the ISB* must
//! live somewhere the unprivileged MPU view permits to read. Kernel flash
//! doesn't, so the kernel copies a four-instruction trampoline
//! (`movs r3,#3; msr control,r3; isb; bx r0`) into user_text RAM and
//! branches into it from privileged code. The trampoline does the drop and
//! then BXes to the user's true entry (passed in R0). Both fetches —
//! trampoline and user — come from user_text, which region 0 allows the
//! unprivileged thread to read.
//!
//! ## Control flow
//!
//! ```text
//!     kernel                                                       user
//!     ──────                                                       ────
//!     launch(prog)
//!       ├─ copy [trampoline | user.text] → USER_TEXT_BASE
//!       ├─ MPU on, pause SysTick, arm timer, snapshot DWT
//!       └─ launch_user_asm:
//!            push {{r4-r11,lr}} on MSP
//!            save MSP+LR to SAVED_CTX
//!            msr psp / msr CONTROL=2 (priv, PSP) / isb
//!            bx TRAMPOLINE  ────────► movs r3,#3 / msr CONTROL,r3 / isb / bx r0 ──► user_entry
//!                                                                                       │
//!                                                                                       ▼
//!                                                                              svc #0/#1 │ fault │ timer match
//!                                                                                       │
//!                                                                              exception (MSP, priv handler mode)
//!                                                                                       │
//!                                                          record status+payload, snap DWT.end
//!                                                                                       │
//!                                                                              longjmp_from_handler:
//!                                                                               restore r4-r11, MSP
//!                                                                               build fake basic frame on MSP
//!                                                                               clear CONTROL.nPRIV
//!                                                                               EXC_RETURN 0xFFFFFFF9
//!                                                                                       │
//!     ◄──── launch_user_asm "returns" ─────────────────────────────────────────────────┘
//!       ├─ MPU off, disarm timer, resume SysTick
//!       └─ read ABORT_STATUS/PAYLOAD/END_CYCLES → AbortResult
//! ```
//!
//! `launch_user_asm` never returns normally; the only path out is the
//! longjmp from an exception handler, which is a hand-rolled
//! `setjmp/longjmp` implemented via a manufactured EXC_RETURN frame on MSP.

use core::arch::naked_asm;
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use cortex_m::peripheral::DWT;
use cortex_m_rt::exception;

use crate::{clock, mpu, time, timeout};

// ---- per-board layout -------------------------------------------------------

// Per-board addresses. The user regions are 8 KiB text + 8 KiB data + 16 KiB
// stack = 32 KiB total, with the stack at the top of the user window. The
// MPU rules require natural power-of-two alignment matching the region
// size — see `mpu::USER_*_SIZE_LOG2` for what each base must satisfy.
//
// On the LPC1768 we put the user regions in the two AHB SRAM banks
// (0x2007_C000 + 0x2008_0000) so the kernel keeps the full 32 KiB local
// SRAM at 0x1000_0000 for itself. User code therefore runs from AHB SRAM
// rather than local SRAM; this is consistent run-to-run, but absolute
// cycle counts differ from any pre-bump measurements.

#[cfg(feature = "lpc1768")]
mod layout {
    pub const USER_TEXT_BASE: u32 = 0x2007_C000; // AHB SRAM0 base
    pub const USER_DATA_BASE: u32 = 0x2007_E000;
    pub const USER_STACK_BASE: u32 = 0x2008_0000; // AHB SRAM1 base (16 KiB-aligned)
    pub const USER_STACK_TOP: u32 = 0x2008_4000;
}

#[cfg(feature = "lm3s6965evb")]
mod layout {
    pub const USER_TEXT_BASE: u32 = 0x2000_8000;
    pub const USER_DATA_BASE: u32 = 0x2000_A000;
    pub const USER_STACK_BASE: u32 = 0x2000_C000;
    pub const USER_STACK_TOP: u32 = 0x2001_0000;
}

pub use layout::*;

pub const USER_TEXT_SIZE: usize = 0x2000;
pub const USER_DATA_SIZE: usize = 0x2000;

// ---- public API -------------------------------------------------------------

pub struct UserProgram {
    pub text: &'static [u8],
    pub entry_offset: u32,
    pub timeout_cycles: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AbortStatus {
    Ok,
    Timeout,
    MemFault,
    BusFault,
    UsageFault,
    LoadError,
}

impl AbortStatus {
    fn from_raw(raw: u32) -> Self {
        match raw {
            STATUS_OK => Self::Ok,
            STATUS_TIMEOUT => Self::Timeout,
            STATUS_MEMFAULT => Self::MemFault,
            STATUS_BUSFAULT => Self::BusFault,
            STATUS_USAGEFAULT => Self::UsageFault,
            _ => Self::LoadError,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AbortResult {
    pub status: AbortStatus,
    /// Reason payload depending on `status`.
    ///
    /// Exit code on [`AbortStatus::Ok`]; fault address (or CFSR snapshot)
    /// on faults; SVC immediate on unknown syscalls; 0 on timeout.
    pub payload: u32,
    /// CPU cycles from launch snapshot to abort handler.
    ///
    /// Useful only when the DWT counter is actually ticking (real hardware;
    /// QEMU's `lm3s6965evb` machine leaves CYCCNT stuck).
    pub cycles: u64,
}

/// Initialize the user-execution subsystem.
///
/// Enables MemManage / BusFault / UsageFault as individually-vectored
/// handlers (they otherwise escalate straight to HardFault and we lose the
/// abort-reason granularity).
pub fn init() {
    // Trampoline length tripwire (see TRAMPOLINE_LEN). Edits to the asm
    // block above without a matching constant update panic here at boot.
    let start = (&raw const __user_trampoline) as *const u8 as usize;
    let end = (&raw const __user_trampoline_end) as usize;
    assert_eq!(
        end - start,
        TRAMPOLINE_LEN,
        "trampoline length drifted; update TRAMPOLINE_LEN in user.rs",
    );

    const SCB_SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
    const SHCSR_MEMFAULTENA: u32 = 1 << 16;
    const SHCSR_BUSFAULTENA: u32 = 1 << 17;
    const SHCSR_USGFAULTENA: u32 = 1 << 18;
    unsafe {
        let shcsr = ptr::read_volatile(SCB_SHCSR);
        ptr::write_volatile(
            SCB_SHCSR,
            shcsr | SHCSR_MEMFAULTENA | SHCSR_BUSFAULTENA | SHCSR_USGFAULTENA,
        );
    }
}

/// Capacity available to user code.
///
/// `USER_TEXT_SIZE` minus the trampoline the kernel installs at the start
/// of the user_text region.
pub const USER_TEXT_CAPACITY: usize = USER_TEXT_SIZE - TRAMPOLINE_LEN;

/// Address inside user_text where the loaded program starts.
///
/// The trampoline occupies `[USER_TEXT_BASE, USER_TEXT_BASE+TRAMPOLINE_LEN)`;
/// user code lives at this address onward.
const USER_CODE_BASE: u32 = USER_TEXT_BASE + TRAMPOLINE_LEN as u32;

/// Run `program` to termination and report what happened.
///
/// Convenience over [`launch_loaded`] that handles the byte-copy itself;
/// used by tests that already have a `&'static [u8]` source slice.
pub fn launch(program: &UserProgram) -> AbortResult {
    if program.text.len() > USER_TEXT_CAPACITY {
        return AbortResult {
            status: AbortStatus::LoadError,
            payload: program.text.len() as u32,
            cycles: 0,
        };
    }
    unsafe {
        ptr::copy_nonoverlapping(
            program.text.as_ptr(),
            USER_CODE_BASE as *mut u8,
            program.text.len(),
        );
        ptr::write_bytes(USER_DATA_BASE as *mut u8, 0, USER_DATA_SIZE);
    }
    launch_loaded(LoadedProgram {
        entry_offset: program.entry_offset,
        text_size: program.text.len() as u32,
        data_size: 0,
        bss_size: 0,
        timeout_cycles: program.timeout_cycles,
    })
}

/// A program whose bytes are already in user_text/user_data.
///
/// Typically because the monolink loader streamed them in. The kernel still
/// owns trampoline installation, .bss zeroing, MPU bring-up, and dispatch.
pub struct LoadedProgram {
    pub entry_offset: u32,
    pub text_size: u32,
    pub data_size: u32,
    pub bss_size: u32,
    pub timeout_cycles: u32,
}

/// Launch a program that's already been loaded into user_text / user_data.
///
/// The caller (typically the monolink loader) must have:
///
///   * written `text_size` bytes starting at `USER_TEXT_BASE + trampoline_len`
///   * written `data_size` bytes starting at `USER_DATA_BASE`
///
/// The kernel installs the trampoline, zeroes the `.bss` window after
/// `.data`, and dispatches.
pub fn launch_loaded(program: LoadedProgram) -> AbortResult {
    if (program.text_size as usize) > USER_TEXT_CAPACITY
        || (program.data_size as usize) + (program.bss_size as usize) > USER_DATA_SIZE
    {
        return AbortResult {
            status: AbortStatus::LoadError,
            payload: program.text_size,
            cycles: 0,
        };
    }
    unsafe {
        ptr::copy_nonoverlapping(
            TRAMPOLINE_BYTES.as_ptr(),
            USER_TEXT_BASE as *mut u8,
            TRAMPOLINE_LEN,
        );
        ptr::write_bytes(
            (USER_DATA_BASE as *mut u8).add(program.data_size as usize),
            0,
            program.bss_size as usize,
        );
    }

    ABORT_STATUS.store(STATUS_OK, Ordering::Relaxed);
    ABORT_PAYLOAD.store(0, Ordering::Relaxed);

    // SysTick is about to be paused, so the software-extended high word
    // can't track wraps during the run. We only use the u32 delta of the
    // raw CYCCNT, which is correct for any run shorter than 2^32 cycles
    // (~44 s at 96 MHz). Timeouts in M1 are far below that.
    let start_low = clock::cycles_now();
    START_CYCLES_LOW.store(start_low, Ordering::Relaxed);
    ABORT_END_LOW.store(start_low, Ordering::Relaxed);

    let systick_was_on = time::pause();
    mpu::enable_user_regions(USER_TEXT_BASE, USER_DATA_BASE, USER_STACK_BASE);
    timeout::arm_in_cycles(program.timeout_cycles);
    USER_RUNNING.store(true, Ordering::Relaxed);

    // The trampoline at user_text_base drops privilege via msr+isb and then
    // BX's to R0 (which the kernel sets to the user's true entry). This is
    // the privileged → unprivileged hand-off; doing it inline in kernel
    // flash fails because the post-ISB fetch lands in flash, which the
    // unprivileged-thread MPU view doesn't permit.
    let user_entry = USER_CODE_BASE.wrapping_add(program.entry_offset) | 1;
    let trampoline_entry = USER_TEXT_BASE | 1;
    let ctx_ptr = SAVED_CTX.0.get();
    unsafe {
        launch_user_asm(user_entry, USER_STACK_TOP, ctx_ptr, trampoline_entry);
    }
    // We only get here via longjmp_from_handler.

    USER_RUNNING.store(false, Ordering::Relaxed);
    timeout::disarm();
    mpu::disable();
    if systick_was_on {
        time::resume();
    }

    let end_low = ABORT_END_LOW.load(Ordering::Relaxed);
    let delta = end_low.wrapping_sub(start_low);

    AbortResult {
        status: AbortStatus::from_raw(ABORT_STATUS.load(Ordering::Relaxed)),
        payload: ABORT_PAYLOAD.load(Ordering::Relaxed),
        cycles: delta as u64,
    }
}

/// Streaming loader for the monolink CHUNK path.
///
/// Tracks where the next byte goes (user_text first up to `text_size`, then
/// user_data). The kernel does the `.bss` zero on launch, not here.
pub struct Loader {
    text_size: u32,
    data_size: u32,
    written: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoaderError {
    /// `text_size` exceeds [`USER_TEXT_CAPACITY`].
    TextTooLarge,
    /// `data_size + bss_size` exceeds [`USER_DATA_SIZE`].
    DataTooLarge,
    /// A chunk wrote past `text_size + data_size`.
    Overflow,
}

impl Loader {
    pub fn new(text_size: u32, data_size: u32, bss_size: u32) -> Result<Self, LoaderError> {
        if text_size as usize > USER_TEXT_CAPACITY {
            return Err(LoaderError::TextTooLarge);
        }
        if (data_size as usize) + (bss_size as usize) > USER_DATA_SIZE {
            return Err(LoaderError::DataTooLarge);
        }
        Ok(Self {
            text_size,
            data_size,
            written: 0,
        })
    }

    /// Append `chunk` bytes at the current write position.
    ///
    /// Returns the new cumulative byte count, or an error if the write
    /// would exceed `text_size + data_size`.
    pub fn append(&mut self, chunk: &[u8]) -> Result<u32, LoaderError> {
        let end = self
            .written
            .checked_add(chunk.len() as u32)
            .ok_or(LoaderError::Overflow)?;
        if end > self.text_size + self.data_size {
            return Err(LoaderError::Overflow);
        }
        for (offset, &byte) in (self.written..).zip(chunk.iter()) {
            unsafe {
                let dst = if offset < self.text_size {
                    (USER_CODE_BASE + offset) as *mut u8
                } else {
                    (USER_DATA_BASE + (offset - self.text_size)) as *mut u8
                };
                ptr::write_volatile(dst, byte);
            }
        }
        self.written = end;
        Ok(end)
    }

    /// Total bytes the host is expected to send (text + data init image).
    pub fn expected_total(&self) -> u32 {
        self.text_size + self.data_size
    }

    /// Cumulative bytes written so far.
    pub fn written(&self) -> u32 {
        self.written
    }
}

// ---- internal state ---------------------------------------------------------

const STATUS_OK: u32 = 0;
const STATUS_TIMEOUT: u32 = 1;
const STATUS_MEMFAULT: u32 = 2;
const STATUS_BUSFAULT: u32 = 3;
const STATUS_USAGEFAULT: u32 = 4;

#[repr(C)]
struct KernelCtx {
    // R4..R11 — callee-saved by the AAPCS.
    r4_r11: [u32; 8],
    /// MSP value just before `push {r4-r11, lr}` in [`launch_user_asm`].
    ///
    /// Restoring this drops the launch frame and any handler activations
    /// above it on the kernel stack.
    msp: u32,
    /// Caller's LR — where [`launch`] expects to "return" to.
    return_lr: u32,
}

#[repr(transparent)]
struct SyncCell(UnsafeCell<KernelCtx>);
unsafe impl Sync for SyncCell {}

static SAVED_CTX: SyncCell = SyncCell(UnsafeCell::new(KernelCtx {
    r4_r11: [0; 8],
    msp: 0,
    return_lr: 0,
}));

static USER_RUNNING: AtomicBool = AtomicBool::new(false);
static ABORT_STATUS: AtomicU32 = AtomicU32::new(0);
static ABORT_PAYLOAD: AtomicU32 = AtomicU32::new(0);
static START_CYCLES_LOW: AtomicU32 = AtomicU32::new(0);
static ABORT_END_LOW: AtomicU32 = AtomicU32::new(0);

// ---- case I/O ---------------------------------------------------------------
//
// `CaseIo` and `DataInitImage` are bag-of-state structs grouped to mirror the
// conceptual unit they belong to. Each field is what an equivalent loose
// `static` would have been — an `UnsafeCell<[u8; _]>` for the byte buffers
// (driven by single-threaded kernel context: case driver in thread mode vs
// SVCall handler, never simultaneously), and `AtomicU32`s for cursors/lengths.
// Methods inline at -O2 to the same accesses, so there's no runtime cost
// versus the loose-statics layout.

/// Per-case input + output capacity, in bytes.
///
/// A single case's accumulated `TYPE_CASE` payload bytes can fill this on
/// the input side, and `write()` calls fill the output side. Hosts whose
/// cases exceed this need to split across multiple `TYPE_CASE` frames —
/// the kernel still rejects anything that overruns this cap.
pub const CASE_IO_LEN: usize = 1024;

/// I/O scratch for a single case.
pub struct CaseIo {
    input: UnsafeCell<[u8; CASE_IO_LEN]>,
    input_len: AtomicU32,
    input_cursor: AtomicU32,
    output: UnsafeCell<[u8; CASE_IO_LEN]>,
    output_len: AtomicU32,
}

// SAFETY: kernel is single-threaded. The case driver in main.rs (thread
// mode) and the SVCall handler (handler mode) are never on the CPU at the
// same time — exceptions return to thread mode before the next syscall —
// so the implicit `&self` borrows can't alias.
unsafe impl Sync for CaseIo {}

/// Overflow returned by [`CaseIo::append_input`] / [`CaseIo::write_from_user`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CaseOverflow {
    /// Total size that would have been needed, in bytes.
    pub needed: u32,
}

impl CaseIo {
    /// Append a chunk to the input buffer.
    pub fn append_input(&self, bytes: &[u8]) -> Result<(), CaseOverflow> {
        let cur = self.input_len.load(Ordering::Relaxed) as usize;
        let needed = cur + bytes.len();
        if needed > CASE_IO_LEN {
            return Err(CaseOverflow {
                needed: needed as u32,
            });
        }
        // SAFETY: see `unsafe impl Sync` above. Single accessor at a time.
        unsafe {
            let dst = (self.input.get() as *mut u8).add(cur);
            ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        }
        self.input_len.store(needed as u32, Ordering::Relaxed);
        Ok(())
    }

    /// Clear the input buffer for the next case.
    pub fn reset_input(&self) {
        self.input_len.store(0, Ordering::Relaxed);
        self.input_cursor.store(0, Ordering::Relaxed);
    }

    /// Reset cursors/lengths for the in-flight case's launch.
    ///
    /// Leaves the input bytes in place; resets the read cursor so the user
    /// program sees them from byte 0, and clears any output state from a
    /// previous case.
    pub fn prepare_for_launch(&self) {
        self.input_cursor.store(0, Ordering::Relaxed);
        self.output_len.store(0, Ordering::Relaxed);
    }

    /// Drain bytes into the user's buffer (SVC #2 / `read`).
    ///
    /// # Safety
    ///
    /// `dst` must be a writable `max`-byte buffer in the active user_data
    /// region — only the syscall handler should call this.
    pub unsafe fn read_into_user(&self, dst: *mut u8, max: usize) -> usize {
        let cursor = self.input_cursor.load(Ordering::Relaxed) as usize;
        let avail = self.input_len.load(Ordering::Relaxed) as usize - cursor;
        let n = avail.min(max);
        // SAFETY: caller guarantees `dst` is valid for `max` bytes; we
        // copy `n ≤ max` from the kernel-side input buffer.
        unsafe {
            let src = (self.input.get() as *const u8).add(cursor);
            ptr::copy_nonoverlapping(src, dst, n);
        }
        self.input_cursor
            .store((cursor + n) as u32, Ordering::Relaxed);
        n
    }

    /// Append bytes from the user (SVC #3 / `write`).
    ///
    /// # Safety
    ///
    /// `src` must be a readable `len`-byte buffer in the active user_data
    /// or user_stack region.
    pub unsafe fn write_from_user(&self, src: *const u8, len: usize) -> Result<(), CaseOverflow> {
        let cur = self.output_len.load(Ordering::Relaxed) as usize;
        let needed = cur + len;
        if needed > CASE_IO_LEN {
            return Err(CaseOverflow {
                needed: needed as u32,
            });
        }
        // SAFETY: caller guarantees `src` is valid for `len` bytes; the
        // destination is our own kernel-side output buffer.
        unsafe {
            let dst = (self.output.get() as *mut u8).add(cur);
            ptr::copy_nonoverlapping(src, dst, len);
        }
        self.output_len.store(needed as u32, Ordering::Relaxed);
        Ok(())
    }

    /// Slice over the bytes the user wrote since the last
    /// [`prepare_for_launch`](Self::prepare_for_launch).
    pub fn output(&self) -> &[u8] {
        let len = self.output_len.load(Ordering::Relaxed) as usize;
        // SAFETY: see `unsafe impl Sync`; called from the case driver
        // *after* the user program has exited or aborted, so no other
        // accessor exists.
        unsafe { core::slice::from_raw_parts(self.output.get() as *const u8, len) }
    }
}

pub static CASE_IO: CaseIo = CaseIo {
    input: UnsafeCell::new([0; CASE_IO_LEN]),
    input_len: AtomicU32::new(0),
    input_cursor: AtomicU32::new(0),
    output: UnsafeCell::new([0; CASE_IO_LEN]),
    output_len: AtomicU32::new(0),
};

/// Snapshot of the loaded `.data` init image, used to reset user state
/// between cases.
pub struct DataInitImage {
    bytes: UnsafeCell<[u8; USER_DATA_SIZE]>,
    data_len: AtomicU32,
    bss_len: AtomicU32,
}

// SAFETY: same single-threaded invariant as [`CaseIo`].
unsafe impl Sync for DataInitImage {}

impl DataInitImage {
    /// Snapshot the currently-loaded `.data` into the kernel-side image.
    ///
    /// Call once at EOF, after the loader has populated `USER_DATA_BASE`.
    pub fn capture(&self, data_size: u32, bss_size: u32) {
        let len = data_size as usize;
        // SAFETY: USER_DATA_BASE points at a `USER_DATA_SIZE`-byte region
        // the loader just wrote; we read up to `data_size` bytes.
        unsafe {
            let src = USER_DATA_BASE as *const u8;
            let dst = self.bytes.get() as *mut u8;
            ptr::copy_nonoverlapping(src, dst, len);
        }
        self.data_len.store(data_size, Ordering::Relaxed);
        self.bss_len.store(bss_size, Ordering::Relaxed);
    }

    /// Restore `.data` from the image and zero `.bss` for the next case.
    pub fn restore_to_user_data(&self) {
        let data_len = self.data_len.load(Ordering::Relaxed) as usize;
        let bss_len = self.bss_len.load(Ordering::Relaxed) as usize;
        // SAFETY: USER_DATA_BASE is the start of the user_data MPU
        // region; we own this RAM and the user isn't running yet.
        unsafe {
            let src = self.bytes.get() as *const u8;
            let dst = USER_DATA_BASE as *mut u8;
            ptr::copy_nonoverlapping(src, dst, data_len);
            ptr::write_bytes(dst.add(data_len), 0, bss_len);
        }
    }
}

pub static DATA_INIT_IMAGE: DataInitImage = DataInitImage {
    bytes: UnsafeCell::new([0; USER_DATA_SIZE]),
    data_len: AtomicU32::new(0),
    bss_len: AtomicU32::new(0),
};

// ---- privilege-drop trampoline ---------------------------------------------
//
// Lives in flash as four Thumb instructions: `movs r3,#3; msr control,r3;
// isb; bx r0`. The kernel copies the bytes to `USER_TEXT_BASE` at every
// launch (region 0 is `priv-RW / unpriv-RO`, so copies work). When the
// privileged kernel branches here, the MSR+ISB drops nPRIV to 1; the
// subsequent BX fetch happens from user_text, which the unprivileged MPU
// view permits, so it doesn't fault the way a drop-then-branch in kernel
// flash would.
//
// `movs` and `bx` are 16-bit Thumb-1; `msr` and `isb` are 32-bit Thumb-2.
// Total: 12 bytes. Editing the instructions means updating `TRAMPOLINE_LEN`;
// `init` checks the symbol diff at boot so a stale constant trips a panic
// instead of going unnoticed. (A `.if`-based assembler-time check would be
// nicer, but LLVM MC refuses to evaluate `. - label` as an absolute
// expression at any opt-level above zero.)

const TRAMPOLINE_LEN: usize = 12;

core::arch::global_asm!(
    r#"
    .section .text.user_trampoline, "ax"
    .syntax unified
    .thumb
    .align 1
    .global __user_trampoline
    .global __user_trampoline_end
__user_trampoline:
        movs    r3, #3
        msr     control, r3
        isb
        bx      r0
__user_trampoline_end:
    "#,
);

unsafe extern "C" {
    static __user_trampoline: [u8; TRAMPOLINE_LEN];
    static __user_trampoline_end: u8;
}

// SAFETY: `__user_trampoline` is a `TRAMPOLINE_LEN`-byte symbol emitted
// by the `global_asm!` above; reading the bytes of executable code in
// our own image is sound.
const TRAMPOLINE_BYTES: &[u8] = unsafe { &__user_trampoline };

// ---- naked entry / resume ---------------------------------------------------

/// Save kernel callee-saves and hand off to the in-RAM privilege-drop trampoline.
///
/// Never returns in the usual sense — control comes back through
/// [`longjmp_from_handler`] from an exception handler.
///
/// AAPCS args: R0 = user entry, R1 = user stack top, R2 = ctx pointer,
/// R3 = trampoline entry (in user_text RAM, Thumb bit set). R0 is passed
/// through untouched so the trampoline can `bx r0` into the user.
#[unsafe(naked)]
unsafe extern "C" fn launch_user_asm(
    _entry: u32,
    _stack_top: u32,
    _ctx: *mut KernelCtx,
    _trampoline: u32,
) {
    naked_asm!(
        // Save callee-saves + LR so the corresponding longjmp can resume
        // launch()'s caller.
        "push  {{r4-r11, lr}}",
        // ctx.r4_r11 = R4..R11.
        "stmia r2, {{r4-r11}}",
        // ctx.msp = MSP just before our push (current SP + 9 words).
        "mov   r12, sp",
        "add   r12, r12, #36",
        "str   r12, [r2, #32]",
        // ctx.return_lr = pushed LR (top of the frame at sp+32).
        "ldr   r12, [sp, #32]",
        "str   r12, [r2, #36]",
        // Set the user PSP and switch SP_active to PSP without dropping
        // privilege yet (nPRIV=0). The trampoline will drop privilege from
        // a location the unprivileged MPU view can still execute.
        "msr   psp, r1",
        "movs  r2, #2", // SPSEL=1, nPRIV=0
        "msr   control, r2",
        "isb",
        // Jump to the trampoline in user_text RAM (still privileged).
        "bx    r3",
    );
}

/// Restore the saved kernel context and exit handler mode with `val` in R0.
///
/// MUST only be called from handler mode, with `ctx` pointing at a context
/// previously populated by [`launch_user_asm`]. The "return" lands at
/// `ctx.return_lr` in thread mode on MSP, callee-saved registers restored.
#[unsafe(naked)]
unsafe extern "C" fn longjmp_from_handler(_val: u32, _ctx: *const KernelCtx) -> ! {
    naked_asm!(
        // Restore R4..R11 from ctx[0..32].
        "ldmia r1, {{r4-r11}}",
        // saved_msp = ctx.msp; saved_lr = ctx.return_lr.
        "ldr   r2, [r1, #32]",
        "ldr   r3, [r1, #36]",
        // Reserve an 8-word basic exception frame at the top of the
        // restored MSP and point MSP there. The CPU will pop it on
        // EXC_RETURN below, landing at the saved LR with R0 = val.
        "subs  r2, r2, #32",
        "msr   msp, r2",
        // Build the frame.
        "str   r0, [r2, #0]", // R0 = val
        "movs  r0, #0",
        "str   r0, [r2, #4]",  // R1
        "str   r0, [r2, #8]",  // R2
        "str   r0, [r2, #12]", // R3
        "str   r0, [r2, #16]", // R12
        "str   r0, [r2, #20]", // LR (unused after return)
        "orr   r3, r3, #1",    // ensure Thumb bit set
        "str   r3, [r2, #24]", // return PC
        "movw  r0, #0x0000",
        "movt  r0, #0x0100", // xPSR = 0x01000000 (T-bit only)
        "str   r0, [r2, #28]",
        // EXC_RETURN won't restore CONTROL.nPRIV — the user-launch path
        // left it at 1 (unprivileged). Without this clear we'd return to
        // unprivileged Thread mode and the very next fetch from kernel
        // flash would fault.
        "movs  r0, #0",
        "msr   control, r0",
        "isb",
        // EXC_RETURN: thread mode, MSP, basic frame (ARMv7-M).
        "movw  lr, #0xFFF9",
        "movt  lr, #0xFFFF",
        "bx    lr",
    );
}

/// Snapshot DWT cycles and longjmp out of handler mode.
///
/// Called by the SVCall / fault / timer-match paths once they've recorded
/// status+payload.
#[inline(always)]
unsafe fn abort_to_kernel() -> ! {
    ABORT_END_LOW.store(DWT::cycle_count(), Ordering::Relaxed);
    let ctx = SAVED_CTX.0.get() as *const KernelCtx;
    unsafe { longjmp_from_handler(0, ctx) }
}

/// Hook for the per-board timeout interrupt.
///
/// If a user program is running, abort it with [`AbortStatus::Timeout`].
/// Otherwise fall through to the regular [`crate::timeout::mark_fired`]
/// path so the M0 timeout tests keep working.
#[inline]
pub fn on_timeout_match() {
    if USER_RUNNING.swap(false, Ordering::Relaxed) {
        ABORT_STATUS.store(STATUS_TIMEOUT, Ordering::Relaxed);
        ABORT_PAYLOAD.store(0, Ordering::Relaxed);
        unsafe { abort_to_kernel() }
    }
    timeout::mark_fired();
}

// ---- exception handlers -----------------------------------------------------

#[exception]
unsafe fn SVCall() {
    // SVCall fires exclusively from user code (kernel never issues `svc`).
    // The user's frame lives on PSP; the SVC immediate is the low byte of
    // the halfword right before the stacked PC.
    let psp: u32;
    unsafe { core::arch::asm!("mrs {}, psp", out(reg) psp, options(nomem, nostack)) };
    let frame = psp as *mut u32;

    let stacked_pc = unsafe { ptr::read_volatile(frame.add(6)) };
    let svc_halfword = unsafe { ptr::read_volatile((stacked_pc - 2) as *const u16) };
    let imm = (svc_halfword & 0xFF) as u8;

    match imm {
        0 => {
            // exit(R0)
            let code = unsafe { ptr::read_volatile(frame) };
            ABORT_STATUS.store(STATUS_OK, Ordering::Relaxed);
            ABORT_PAYLOAD.store(code, Ordering::Relaxed);
            USER_RUNNING.store(false, Ordering::Relaxed);
            unsafe { abort_to_kernel() }
        }
        1 => {
            // time_cycles -> R0:R1 = u64 cycles since program start
            let now = DWT::cycle_count();
            let start = START_CYCLES_LOW.load(Ordering::Relaxed);
            let delta = now.wrapping_sub(start) as u64;
            unsafe {
                ptr::write_volatile(frame.add(0), delta as u32);
                ptr::write_volatile(frame.add(1), (delta >> 32) as u32);
            }
            // Normal exception return: cortex-m-rt's trampoline bxs the
            // EXC_RETURN value that was loaded into LR on entry.
        }
        2 => {
            // read(buf_ptr: *mut u8, max: u32) -> u32 (bytes read)
            let dst = unsafe { ptr::read_volatile(frame.add(0)) } as *mut u8;
            let max = unsafe { ptr::read_volatile(frame.add(1)) } as usize;
            // SAFETY: dst lives in the user_data MPU region; kernel
            // privileged-mode writes through it are allowed.
            let n = unsafe { CASE_IO.read_into_user(dst, max) };
            unsafe { ptr::write_volatile(frame.add(0), n as u32) };
        }
        3 => {
            // write(buf_ptr: *const u8, len: u32)
            let src = unsafe { ptr::read_volatile(frame.add(0)) } as *const u8;
            let len = unsafe { ptr::read_volatile(frame.add(1)) } as usize;
            // SAFETY: src lives in a user-readable MPU region; the read
            // happens from privileged mode here.
            match unsafe { CASE_IO.write_from_user(src, len) } {
                Ok(()) => { /* no return value */ }
                Err(overflow) => {
                    ABORT_STATUS.store(STATUS_USAGEFAULT, Ordering::Relaxed);
                    ABORT_PAYLOAD.store(overflow.needed, Ordering::Relaxed);
                    USER_RUNNING.store(false, Ordering::Relaxed);
                    unsafe { abort_to_kernel() }
                }
            }
        }
        _ => {
            // Unknown SVC immediate — treat as a usage fault.
            ABORT_STATUS.store(STATUS_USAGEFAULT, Ordering::Relaxed);
            ABORT_PAYLOAD.store(imm as u32, Ordering::Relaxed);
            USER_RUNNING.store(false, Ordering::Relaxed);
            unsafe { abort_to_kernel() }
        }
    }
}

#[exception]
unsafe fn MemoryManagement() {
    const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
    const MMFAR: *const u32 = 0xE000_ED34 as *const u32;
    const MMFSR_MMARVALID: u32 = 1 << 7;

    let cfsr = unsafe { ptr::read_volatile(CFSR) };
    let mmfsr = cfsr & 0xFF;
    let payload = if mmfsr & MMFSR_MMARVALID != 0 {
        unsafe { ptr::read_volatile(MMFAR) }
    } else {
        cfsr
    };
    unsafe { ptr::write_volatile(CFSR, cfsr) }; // write-1-to-clear

    if USER_RUNNING.swap(false, Ordering::Relaxed) {
        ABORT_STATUS.store(STATUS_MEMFAULT, Ordering::Relaxed);
        ABORT_PAYLOAD.store(payload, Ordering::Relaxed);
        unsafe { abort_to_kernel() }
    }
    panic!("kernel MemManage fault: cfsr={cfsr:#010x} mmfar={payload:#010x}");
}

#[exception]
unsafe fn BusFault() {
    const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
    const BFAR: *const u32 = 0xE000_ED38 as *const u32;
    const BFSR_BFARVALID: u32 = 1 << 15;

    let cfsr = unsafe { ptr::read_volatile(CFSR) };
    let payload = if cfsr & BFSR_BFARVALID != 0 {
        unsafe { ptr::read_volatile(BFAR) }
    } else {
        cfsr
    };
    unsafe { ptr::write_volatile(CFSR, cfsr) };

    if USER_RUNNING.swap(false, Ordering::Relaxed) {
        ABORT_STATUS.store(STATUS_BUSFAULT, Ordering::Relaxed);
        ABORT_PAYLOAD.store(payload, Ordering::Relaxed);
        unsafe { abort_to_kernel() }
    }
    panic!("kernel BusFault: cfsr={cfsr:#010x} bfar={payload:#010x}");
}

#[exception]
unsafe fn UsageFault() {
    const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
    let cfsr = unsafe { ptr::read_volatile(CFSR) };
    unsafe { ptr::write_volatile(CFSR, cfsr) };

    if USER_RUNNING.swap(false, Ordering::Relaxed) {
        ABORT_STATUS.store(STATUS_USAGEFAULT, Ordering::Relaxed);
        ABORT_PAYLOAD.store(cfsr, Ordering::Relaxed);
        unsafe { abort_to_kernel() }
    }
    panic!("kernel UsageFault: cfsr={cfsr:#010x}");
}
