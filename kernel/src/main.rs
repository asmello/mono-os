#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;

#[cfg(not(test))]
#[entry]
fn main() -> ! {
    use kernel::m1_demos;
    use kernel::user::{AbortResult, UserProgram};

    kernel::init();
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::clock::init(p.DCB, p.DWT);
    kernel::time::init(p.SYST);
    kernel::timeout::init();
    kernel::println!("hello, world");

    // Drive each M1 demo program once and report the result over UART so a
    // human watching the console (tio /dev/cu.usbmodem*) can confirm the
    // sandbox plumbing on real hardware. Same suite that runs as
    // `cargo test --test m1_user` under QEMU, minus the semihosting exit.
    fn run_demo(name: &str, program: UserProgram) {
        let AbortResult {
            status,
            payload,
            cycles,
        } = kernel::user::launch(&program);
        kernel::println!(
            "demo {name:<16} status={status:?} payload={payload:#010x} cycles={cycles}"
        );
    }
    run_demo("exit_42", m1_demos::exit_42());
    run_demo("reads_cycles", m1_demos::reads_cycles());
    run_demo("touches_kernel", m1_demos::touches_kernel());
    run_demo("executes_data", m1_demos::executes_data());
    run_demo("infinite_loop", m1_demos::infinite_loop());
    kernel::println!("M1 demos complete");

    let mut last_second = 0u32;
    let mut last_cycles = kernel::clock::cycles_now_u64();
    loop {
        let secs = kernel::time::ticks() / 1000;
        if secs > last_second {
            last_second = secs;
            let now_cycles = kernel::clock::cycles_now_u64();
            let delta = now_cycles - last_cycles;
            last_cycles = now_cycles;
            kernel::println!("tick {secs} ({delta} cycles)");
        }
        cortex_m::asm::wfi();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::println!();
    kernel::println!("KERNEL PANIC");
    kernel::println!("{info}");
    kernel::wfi_loop();
}

#[cfg(test)]
#[entry]
fn main() -> ! {
    kernel::init();
    test_main();
    kernel::wfi_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}

#[cfg(test)]
mod tests {
    #[test_case]
    #[allow(clippy::eq_op)]
    fn trivial_assertion() {
        assert_eq!(1, 1);
    }
}
