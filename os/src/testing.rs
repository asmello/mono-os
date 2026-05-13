use crate::{serial_print, serial_println};

pub fn test_runner(tests: &[&dyn Testable]) {
    use crate::{QemuExitCode, exit_qemu};

    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }

    exit_qemu(QemuExitCode::Success);
}

pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        #[cfg(feature = "vga-text")]
        crate::vga_buffer::WRITER.lock().clear();
        self();
        serial_println!("[ok]");
    }
}
