// AArch64 Linux hello world — uses sys_write then sys_exit
//
// To assemble and run:
//   as -o hello.o hello.s
//   ld -o hello hello.o
//   ./hello

.section .rodata        // read-only data section — the linker will refuse to
                        // let code write here (good: protects string literals)
msg:
    .ascii "Hello, world!\n"    // raw bytes, no null terminator needed
                                // (we pass an explicit length to sys_write)
msglen = . - msg                // assembler-time constant: current position
                                // minus msg's position = byte length of string
                                // no runtime cost — assembler computes it

.section .text
.global _start

_start:
    // ── syscall 1: write(fd, buf, count) ──────────────────────────────────
    //
    // C signature: ssize_t write(int fd, const void *buf, size_t count)
    // Linux AArch64 syscall number: 64
    //
    // Arguments follow the Linux AArch64 ABI convention:
    //   x8 = syscall number
    //   x0 = arg1 (fd)
    //   x1 = arg2 (buf pointer)
    //   x2 = arg3 (byte count)

    mov x8, #64         // sys_write syscall number
                        // note: Linux AArch64 uses 64, but x86-64 Linux uses 1
                        // same syscall, same kernel, different numbering scheme
                        // — the numbers are just indices into each arch's
                        // own syscall table, defined in the kernel source

    mov x0, #1          // arg1: fd = 1 (stdout)
                        // 0 = stdin, 1 = stdout, 2 = stderr
                        // these are also conventions — the shell sets them up
                        // before your process starts

    adr x1, msg         // arg2: pointer to our string
                        //
                        // 'adr' = Address of label, PC-relative.
                        // It encodes the offset from the current instruction
                        // to 'msg' and adds it to the program counter at
                        // runtime — so it works regardless of where the OS
                        // loads the binary in memory.
                        //
                        // You can't use 'mov x1, msg' for addresses —
                        // mov only handles immediates that fit in the
                        // instruction encoding. For addresses you need
                        // adr (short range) or adrp+add (full 4GB range).

    mov x2, msglen      // arg3: number of bytes to write
                        // the assembler substituted the constant value here
                        // — no pointer, no strlen(), just a number baked in
                        // at assemble time

    svc #0              // enter kernel — dispatch to sys_write
                        // on return, x0 contains the number of bytes written
                        // (or a negative errno value on error, which we ignore)

    // ── syscall 2: exit(0) ────────────────────────────────────────────────
    //
    // We must explicitly call sys_exit — unlike a C program there is no
    // runtime wrapping main() that calls exit() for us. Without this, the
    // CPU would fall through into whatever bytes follow, which would likely
    // trigger an illegal instruction fault.

    mov x8, #93         // sys_exit syscall number (Linux AArch64 convention)

    mov x0, #0          // exit status 0 — "success" by Unix convention
                        // the shell reads this via $? or waitpid()

    svc #0              // enter kernel — process exits, never returns
