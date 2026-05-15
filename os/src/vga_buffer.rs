use core::fmt;
use spin::{Lazy, Mutex};
use volatile::Volatile;
use yore::code_pages::CP437G;

pub static WRITER: Lazy<Mutex<Writer>> = Lazy::new(|| {
    Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::White, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    })
});

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        if self.column_position >= BUFFER_WIDTH {
            self.new_line();
        }

        let row = BUFFER_HEIGHT - 1;
        let col = self.column_position;

        self.buffer.chars[row][col].write(ScreenChar {
            character: byte,
            color_code: self.color_code,
        });
        self.column_position += 1;
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    pub fn clear(&mut self) {
        for i in 0..BUFFER_HEIGHT {
            self.clear_row(i);
        }
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => self.new_line(),
                '\t' => self.write_string("     "),
                c => {
                    let byte = CP437G.encode_char(c).unwrap_or(0xfe);
                    self.write_byte(byte);
                }
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        crate::vga_buffer::WRITER.lock().write_fmt(args).unwrap();
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        vga_buffer::{BUFFER_HEIGHT, BUFFER_WIDTH, WRITER},
        vga_println,
    };

    #[test_case]
    fn vga_println_simple() {
        vga_println!("println_simple output");
    }

    #[test_case]
    fn vga_println_many() {
        for _ in 0..200 {
            vga_println!("test_println_many output");
        }
    }

    #[test_case]
    fn vga_println_output() {
        use core::fmt::Write;
        use yore::code_pages::CP437G;

        let s = "Some test string that fits on a single line";
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut writer = WRITER.lock();
            writeln!(writer, "\n{s}").unwrap();
            for (i, c) in s.chars().enumerate() {
                let screen_char = writer.buffer.chars[BUFFER_HEIGHT - 2][i].read();
                assert_eq!(CP437G.decode_byte(screen_char.character), c);
            }
        });
    }

    #[test_case]
    fn clear() {
        WRITER.lock().clear();
        for i in 0..BUFFER_HEIGHT {
            for j in 0..BUFFER_WIDTH {
                let screen_char = WRITER.lock().buffer.chars[i][j].read();
                assert_eq!(screen_char.character, b' ');
            }
        }
    }
}
