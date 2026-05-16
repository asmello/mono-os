use crate::{gdt, print};
use spin::{Lazy, Mutex};
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
    idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
    idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
    idt
});

pub(crate) mod pic {
    use pic8259::ChainedPics;
    use spin::Mutex;

    pub(super) const PIC_1_OFFSET: u8 = 32;
    pub(super) const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

    pub(super) static PICS: Mutex<ChainedPics> =
        Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

    pub(crate) fn init() {
        unsafe {
            PICS.lock().initialize();
        }
    }
}

#[allow(unused)]
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    #[cfg(test)]
    crate::tprintln!("EXCEPTION: BREAKPOINT\n{stack_frame:#?}");
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{stack_frame:#?}");
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        pic::PICS
            .lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};
    use x86_64::instructions::port::Port;

    static KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    ));

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode)
        && let Some(key) = keyboard.process_keyevent(key_event)
    {
        match key {
            DecodedKey::Unicode(ch) => print!("{ch}"),
            DecodedKey::RawKey(key) => print!("{key:?}"),
        }
    }

    unsafe {
        pic::PICS
            .lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = pic::PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

pub fn init_idt() {
    IDT.load();
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn test_breakpoint_exception() {
        x86_64::instructions::interrupts::int3();
    }
}
