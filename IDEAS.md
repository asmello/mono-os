# Project Ideas & Roadmap

This file tracks future goals and experiments for the Umami OS project.

## Hardware & Simulation
- [ ] **Bare Metal on Spike**: Run the Spike simulator directly with a custom, standalone bare-metal kernel (no Proxy Kernel).
    - Requires implementing a custom `crt0.s` (startup code).
    - Requires a custom linker script to place code at the correct memory address.
    - Requires direct interaction with Spike's HTIF (Host-Target Interface) for console I/O.
- [ ] **Umami OS as Spike Kernel**: Use the code in `os/` as the primary kernel for Spike.
    - Transition the current Rust OS implementation to handle Supervisor mode traps.
    - Implement basic memory mapping and device tree parsing.

## Hardware & Booting Research
- [ ] **Physical Chip Boot Sequence**: Research how real RISC-V/ARM hardware boots compared to a simulator.
    - Study the **ELF to BIN** conversion process (`objcopy`).
    - Implement a full **CRT0 (C Runtime Initialization)**:
        - Copying the `.data` section from Flash to RAM.
        - Zeroing the `.bss` section (uninitialized globals).
        - Setting up the initial Stack Pointer (SP) manually.
    - Research **Linker Scripts (`layout.ld`)** for mapping code to physical Flash/RAM addresses.

## Application Layer
- [ ] Explore running the `hello-*` apps as userspace processes on Umami OS instead of using `pk`.
