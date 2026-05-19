//! Run one untrusted program in unprivileged thread mode on PSP, under
//! MPU-enforced memory protection and a hardware timeout. Terminate on
//! `exit` syscall, fault, or deadline; report status + cycle count.
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

#[cfg(feature = "lpc1768")]
mod layout {
    pub const USER_TEXT_BASE: u32 = 0x1000_4000;
    pub const USER_DATA_BASE: u32 = 0x1000_5000;
    pub const USER_STACK_BASE: u32 = 0x1000_6000;
    pub const USER_STACK_TOP: u32 = 0x1000_8000;
}

#[cfg(feature = "lm3s6965evb")]
mod layout {
    pub const USER_TEXT_BASE: u32 = 0x2000_C000;
    pub const USER_DATA_BASE: u32 = 0x2000_D000;
    pub const USER_STACK_BASE: u32 = 0x2000_E000;
    pub const USER_STACK_TOP: u32 = 0x2001_0000;
}

pub use layout::*;

pub const USER_TEXT_SIZE: usize = 0x1000;
pub const USER_DATA_SIZE: usize = 0x1000;

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
    /// Exit code on [`AbortStatus::Ok`]; fault address (or CFSR snapshot) on
    /// faults; SVC immediate on unknown syscalls; 0 on timeout.
    pub payload: u32,
    /// CPU cycles from launch snapshot to abort handler. Useful only when the
    /// DWT counter is actually ticking (real hardware; QEMU's
    /// `lm3s6965evb` machine leaves CYCCNT stuck).
    pub cycles: u64,
}

/// Initialize the user-execution subsystem. Enables MemManage / BusFault /
/// UsageFault as individually-vectored handlers (they otherwise escalate
/// straight to HardFault and we lose the abort-reason granularity).
pub fn init() {
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

/// Run `program` to termination and report what happened.
pub fn launch(program: &UserProgram) -> AbortResult {
    if program.text.len() > USER_TEXT_SIZE {
        return AbortResult {
            status: AbortStatus::LoadError,
            payload: program.text.len() as u32,
            cycles: 0,
        };
    }

    let trampoline = trampoline_bytes();
    let total_len = trampoline.len() + program.text.len();
    if total_len > USER_TEXT_SIZE {
        return AbortResult {
            status: AbortStatus::LoadError,
            payload: total_len as u32,
            cycles: 0,
        };
    }
    unsafe {
        ptr::copy_nonoverlapping(
            trampoline.as_ptr(),
            USER_TEXT_BASE as *mut u8,
            trampoline.len(),
        );
        ptr::copy_nonoverlapping(
            program.text.as_ptr(),
            (USER_TEXT_BASE as *mut u8).add(trampoline.len()),
            program.text.len(),
        );
        ptr::write_bytes(USER_DATA_BASE as *mut u8, 0, USER_DATA_SIZE);
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
    let user_entry = USER_TEXT_BASE
        .wrapping_add(trampoline.len() as u32)
        .wrapping_add(program.entry_offset)
        | 1;
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

// ---- privilege-drop trampoline ---------------------------------------------
//
// Lives in flash as four Thumb instructions: `movs r3,#3; msr control,r3;
// isb; bx r0`. The kernel copies the bytes to `USER_TEXT_BASE` at every
// launch (region 0 is `priv-RW / unpriv-RO`, so copies work). When the
// privileged kernel branches here, the MSR+ISB drops nPRIV to 1; the
// subsequent BX fetch happens from user_text, which the unprivileged MPU
// view permits, so it doesn't fault the way a drop-then-branch in kernel
// flash would.

core::arch::global_asm!(
    r#"
    .section .text.user_trampoline, "ax"
    .syntax unified
    .thumb
    .align 1
    .global __user_trampoline_start
    .global __user_trampoline_end
__user_trampoline_start:
        movs    r3, #3
        msr     control, r3
        isb
        bx      r0
__user_trampoline_end:
    "#
);

unsafe extern "C" {
    static __user_trampoline_start: u8;
    static __user_trampoline_end: u8;
}

fn trampoline_bytes() -> &'static [u8] {
    unsafe {
        let s = &__user_trampoline_start as *const u8;
        let e = &__user_trampoline_end as *const u8;
        core::slice::from_raw_parts(s, e.offset_from(s) as usize)
    }
}

// ---- naked entry / resume ---------------------------------------------------

/// Save kernel callee-saves and hand off to the in-RAM trampoline that
/// drops privilege. Never returns in the usual sense — control comes back
/// through [`longjmp_from_handler`] from an exception handler.
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

/// Snapshot DWT cycles and longjmp out of handler mode. Called by the
/// SVCall / fault / timer-match paths once they've recorded status+payload.
#[inline(always)]
unsafe fn abort_to_kernel() -> ! {
    ABORT_END_LOW.store(DWT::cycle_count(), Ordering::Relaxed);
    let ctx = SAVED_CTX.0.get() as *const KernelCtx;
    unsafe { longjmp_from_handler(0, ctx) }
}

/// Hook for the per-board timeout interrupt: if a user program is running,
/// abort it with [`AbortStatus::Timeout`]. Otherwise fall through to the
/// regular [`crate::timeout::mark_fired`] path so the M0 timeout tests keep
/// working.
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
