use crate::gdt;
use spin::Lazy;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    {
        let entry_opts = idt.double_fault.set_handler_fn(double_fault_handler);
        unsafe {
            entry_opts.set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
    }
    idt
});

pub fn init_idt() {
    IDT.load();
}

#[allow(unused)]
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    #[cfg(test)]
    crate::tprintln!("EXCEPTION: BREAKPOINT\n{stack_frame:#?}");
}

#[allow(unused)]
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{stack_frame:#?}");
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn test_breakpoint_exception() {
        x86_64::instructions::interrupts::int3();
    }
}
