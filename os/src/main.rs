#![no_std]
#![no_main]

mod vga_buffer;

#[panic_handler]
fn panc(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");

    loop {}
}

#[unsafe(no_mangle)]
#[allow(clippy::empty_loop)]
pub extern "C" fn _start() -> ! {
    println!("Hello ☺◙♪\n2nd line\n");

    loop {}
}
