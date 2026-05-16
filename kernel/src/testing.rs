//! Custom test harness.
//!
//! `cargo test` builds each test binary, runs it under QEMU via the configured
//! `runner`, and exits via semihosting `debug::exit`. Test output goes over the
//! same UART used at runtime; the semihosting exit code is the only channel
//! cargo actually inspects.

use cortex_m_semihosting::debug;

pub trait Testable {
    fn run(&self, target_width: usize);
    fn name(&self) -> &'static str;
}

impl<T: Fn()> Testable for T {
    fn run(&self, target_width: usize) {
        let name = self.name();
        let padding = target_width.saturating_sub(name.len());
        crate::print!("{name}...");
        self();
        crate::println!("{:.>1$}", "[ok]", padding);
    }

    fn name(&self) -> &'static str {
        core::any::type_name::<T>()
    }
}

pub fn test_runner(tests: &[&dyn Testable]) -> ! {
    let max_len = tests
        .iter()
        .map(|t| t.name().len())
        .max()
        .unwrap_or_default();
    let target_width = max_len.div_ceil(20) * 20;

    crate::println!("running {} tests", tests.len());
    for test in tests {
        test.run(target_width);
    }

    debug::exit(debug::EXIT_SUCCESS);
    crate::wfi_loop();
}

pub fn test_panic_handler(info: &core::panic::PanicInfo) -> ! {
    crate::println!(" [failed]");
    crate::println!("error: {info}");
    debug::exit(debug::EXIT_FAILURE);
    crate::wfi_loop();
}
