use crate::{
    qemu::{QemuExitCode, exit_qemu},
    serial_print, serial_println,
};
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;

pub fn test_runner(tests: &[&dyn Testable]) -> ! {
    use crate::qemu::{QemuExitCode, exit_qemu};

    let max_len = tests
        .iter()
        .map(|t| t.name().len())
        .max()
        .unwrap_or_default();
    let target_width = max_len.div_ceil(20) * 20;

    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run(target_width);
    }

    exit_qemu(QemuExitCode::Success);
}

pub trait Testable {
    fn run(&self, target_width: usize);
    fn name(&self) -> &'static str;
}

impl<T: Fn()> Testable for T {
    fn run(&self, target_width: usize) {
        let name = self.name();
        let padding = target_width - name.len();
        serial_print!("{name}...");
        self();
        serial_println!("{:.>1$}", "[ok]", padding);
    }

    fn name(&self) -> &'static str {
        core::any::type_name::<T>()
    }
}

#[allow(clippy::empty_loop)]
pub fn test_panic_handler(info: &core::panic::PanicInfo) -> ! {
    serial_println!(" [failed]\n");
    serial_println!("Error: {info}\n");
    serial_println!("Output:\n{}", OUTPUT_CAPTURE.lock());
    exit_qemu(QemuExitCode::Failed);
}

pub(crate) static OUTPUT_CAPTURE: Mutex<OutputCapture> = Mutex::new(OutputCapture::new());

pub struct OutputCapture {
    buffer: Vec<u8>,
}

impl OutputCapture {
    const fn new() -> Self {
        Self { buffer: Vec::new() }
    }
}

impl fmt::Write for OutputCapture {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.buffer.extend(s.bytes());
        Ok(())
    }
}

impl fmt::Display for OutputCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            str::from_utf8(&self.buffer).unwrap_or("<invalid utf-8>")
        )
    }
}
