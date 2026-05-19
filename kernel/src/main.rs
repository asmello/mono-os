#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

use cortex_m_rt::entry;

#[cfg(not(test))]
#[entry]
fn main() -> ! {
    use kernel::board::{Board, Mcu};
    use kernel::link::{
        self, ExecHeader, Frame, Parser, Poll, ResultFrame, TYPE_CHUNK, TYPE_EOF, TYPE_START_LOAD,
    };
    use kernel::user::{self, AbortResult, AbortStatus, LoadedProgram, Loader};

    kernel::init();
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::clock::init(p.DCB, p.DWT);
    kernel::time::init(p.SYST);
    kernel::timeout::init();
    kernel::println!("monolink ready");

    let cclk_hz = <Mcu as Board>::CCLK_HZ;
    let cycles_per_ms = cclk_hz / 1000;

    let mut parser = Parser::new();
    let mut session: Option<(Loader, ExecHeader)> = None;

    loop {
        match parser.poll() {
            Poll::Pending => {
                cortex_m::asm::wfi();
            }
            Poll::Bad(reason) => {
                kernel::println!("link nak: {reason:?}");
                link::send_nak();
                session = None;
            }
            Poll::Frame(frame) => {
                session = handle_frame(frame, session, cycles_per_ms, cclk_hz);
            }
        }
    }

    fn handle_frame(
        frame: Frame<'_>,
        session: Option<(Loader, ExecHeader)>,
        cycles_per_ms: u32,
        cclk_hz: u32,
    ) -> Option<(Loader, ExecHeader)> {
        match frame.kind {
            TYPE_START_LOAD => start_load(frame.payload),
            TYPE_CHUNK => append_chunk(frame.payload, session),
            TYPE_EOF => {
                finish_load(session, cycles_per_ms, cclk_hz);
                None
            }
            other => {
                kernel::println!("link nak: unknown type {other:#04x}");
                link::send_nak();
                session
            }
        }
    }

    fn start_load(payload: &[u8]) -> Option<(Loader, ExecHeader)> {
        let header = match ExecHeader::parse(payload) {
            Some(h) => h,
            None => {
                kernel::println!("link nak: header size {}", payload.len());
                link::send_nak();
                return None;
            }
        };
        if header.magic != ExecHeader::MAGIC || header.version != ExecHeader::VERSION {
            kernel::println!(
                "link nak: bad magic/version (magic={:?} version={})",
                header.magic,
                header.version
            );
            link::send_nak();
            return None;
        }
        match Loader::new(header.text_size, header.data_size, header.bss_size) {
            Ok(loader) => {
                link::send_ack();
                Some((loader, header))
            }
            Err(e) => {
                kernel::println!("link nak: loader rejected sizes: {e:?}");
                link::send_nak();
                None
            }
        }
    }

    fn append_chunk(
        payload: &[u8],
        session: Option<(Loader, ExecHeader)>,
    ) -> Option<(Loader, ExecHeader)> {
        let (mut loader, header) = match session {
            Some(s) => s,
            None => {
                kernel::println!("link nak: chunk without start_load");
                link::send_nak();
                return None;
            }
        };
        match loader.append(payload) {
            Ok(_) => {
                link::send_ack();
                Some((loader, header))
            }
            Err(e) => {
                kernel::println!("link nak: chunk append failed: {e:?}");
                link::send_nak();
                None
            }
        }
    }

    fn finish_load(session: Option<(Loader, ExecHeader)>, cycles_per_ms: u32, cclk_hz: u32) {
        let (loader, header) = match session {
            Some(s) => s,
            None => {
                kernel::println!("link nak: eof without start_load");
                link::send_nak();
                return;
            }
        };
        if loader.written() != loader.expected_total() {
            kernel::println!(
                "link nak: eof at {}/{} bytes",
                loader.written(),
                loader.expected_total()
            );
            link::send_nak();
            return;
        }
        link::send_ack();

        let AbortResult {
            status,
            payload,
            cycles,
        } = user::launch_loaded(LoadedProgram {
            entry_offset: header.entry_offset,
            text_size: header.text_size,
            data_size: header.data_size,
            bss_size: header.bss_size,
            timeout_cycles: header.timeout_ms.saturating_mul(cycles_per_ms),
        });
        let status_byte = match status {
            AbortStatus::Ok => link::STATUS_OK,
            AbortStatus::Timeout => link::STATUS_TIMEOUT,
            AbortStatus::MemFault => link::STATUS_MEMFAULT,
            AbortStatus::BusFault => link::STATUS_BUSFAULT,
            AbortStatus::UsageFault => link::STATUS_USAGEFAULT,
            AbortStatus::LoadError => link::STATUS_LOAD_ERROR,
        };
        let mut frame_bytes = [0u8; ResultFrame::SIZE];
        ResultFrame {
            status: status_byte,
            _pad: [0; 3],
            exit_code: payload,
            cycles,
            cclk_hz,
        }
        .write_le(&mut frame_bytes);
        link::send(link::TYPE_RESULT, &frame_bytes);
        kernel::println!("ran: status={status:?} exit={payload:#010x} cycles={cycles}");
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
