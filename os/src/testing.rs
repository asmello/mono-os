use crate::{
    qemu::{QemuExitCode, exit_qemu},
    serial_print, serial_println,
};
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
        #[cfg(feature = "vga-text")]
        crate::vga_buffer::WRITER.lock().clear();

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
    buffer: [u8; Self::BUF_SIZE],
    index: usize,
    filled: bool,
}

impl OutputCapture {
    const BUF_SIZE: usize = 1024 * 1024;

    const fn new() -> Self {
        Self {
            buffer: [0; Self::BUF_SIZE],
            index: 0,
            filled: false,
        }
    }
}

impl fmt::Write for OutputCapture {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if self.index + len < Self::BUF_SIZE {
            self.buffer[self.index..self.index + len].copy_from_slice(bytes);
            self.index += len;
        } else {
            let tail_len = Self::BUF_SIZE - self.index;
            self.buffer[self.index..Self::BUF_SIZE].copy_from_slice(&bytes[..tail_len]);
            let head_len = len - tail_len;
            self.buffer[..head_len].copy_from_slice(&bytes[tail_len..]);
            self.index = head_len;
            self.filled = true;
        }
        Ok(())
    }
}

impl fmt::Display for OutputCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.filled {
            let head = str::from_utf8(&self.buffer[..self.index]).unwrap();
            let tail = str::from_utf8(&self.buffer[self.index..]).unwrap();
            write!(f, "{tail}{head}")
        } else {
            write!(f, "{}", str::from_utf8(&self.buffer[..self.index]).unwrap())
        }
    }
}
