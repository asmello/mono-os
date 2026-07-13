section .text
global _start

_start:
    mov eax, 60     ; sys_exit syscall number
    mov edi, 42     ; status = 42
    syscall