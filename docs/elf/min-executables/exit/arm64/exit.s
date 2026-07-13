// Minimal AArch64 Linux program: just calls exit(42)
//
// To assemble and run (inside a Linux ARM64 environment):
//   as -o exit.o exit.s
//   ld -o exit exit.o
//   ./exit; echo $?

.section .text      // everything below goes into the executable code section
.global _start      // expose _start so the linker can find the entry point
                    // (the C runtime would normally call main, we bypass all of that)

_start:
    // To make a syscall on AArch64 Linux, the convention is:
    //
    //   x8  = syscall number
    //   x0  = first argument  (also receives the return value afterwards)
    //   x1  = second argument
    //   x2  = third argument
    //   ... up to x5 for the sixth argument
    //
    // This layout is a LINUX ABI CONVENTION, not something the CPU enforces.
    // The 'svc' instruction doesn't know or care about x8 — it simply triggers
    // a CPU exception that hands control to the kernel. The kernel's exception
    // handler then *chooses* to read x8 for the syscall number.
    //
    // If you wrote your own OS, you could define your own convention:
    //   - syscall number in x7, x15, or anywhere you like
    //   - arguments in any order
    //   - triggered by a different exception mechanism entirely
    //
    // The reason Linux chose x8 (rather than x0) is practical: it keeps
    // arguments contiguous in x0-x5 so a 6-argument syscall doesn't need
    // to evict one of them to make room for the syscall number.

    mov x8, #93     // load syscall number 93 (sys_exit) into x8
                    // 93 is also a Linux convention. It's just an index into
                    // the kernel's syscall table. On macOS/Darwin the syscall
                    // numbers are completely different even on the same hardware.

    mov x0, #42     // first argument to sys_exit: the exit status code
                    // x0 plays double duty. It's arg1 on the way in,
                    // and the return value on the way out (not relevant here
                    // since exit never returns)

    svc #0          // Supervisor Call: triggers a CPU exception, switching
                    // from EL0 (user mode) to EL1 (kernel mode).
                    //
                    // The #0 is a 16-bit immediate baked into the instruction
                    // encoding. Linux ignores it. The kernel identifies the
                    // request via x8 instead. Some other OSes (e.g. older ARM
                    // kernels, or bare-metal RTOS systems) do use the svc
                    // immediate as the syscall number, which is why the field
                    // exists. On Linux it is always #0 by convention.
                    //
                    // After svc, the CPU jumps to the kernel's exception vector.
                    // The kernel reads x8, dispatches to sys_exit, and this
                    // process ceases to exist, execution never returns here.
