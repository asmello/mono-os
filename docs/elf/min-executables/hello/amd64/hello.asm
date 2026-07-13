; x86-64 Linux hello world: write(1, msg, len) then exit(0)
;
; To assemble and run:
;   nasm -f elf64 hello.asm -o hello.o
;   ld hello.o -o hello
;   ./hello

section .rodata
msg: db "Hello, world!", 10
msglen equ $ - msg

section .text
global _start

_start:
    mov eax, 1          ; sys_write
    mov edi, 1          ; fd = stdout
    lea rsi, [rel msg]  ; buf = &msg
    mov edx, msglen     ; count = len(msg)
    syscall

    mov eax, 60         ; sys_exit
    xor edi, edi        ; status = 0
    syscall
