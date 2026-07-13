#![no_std]
#![no_main]

#[panic_handler]
fn panc(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

static HELLO: &[u8] = b"Hello World!";

#[unsafe(no_mangle)]
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    let vga_buffer = 0xb8000 as *mut u8;

    for (i, &byte) in HELLO.iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xb;
        }
    }

    loop {}
}
