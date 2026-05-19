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
        self, BadReason, ExecHeader, Frame, NakReason, Parser, Poll, ResultFrame, TYPE_CHUNK,
        TYPE_EOF, TYPE_START_LOAD,
    };
    use kernel::user::{self, AbortResult, AbortStatus, LoadedProgram, Loader, LoaderError};

    kernel::init();
    let p = cortex_m::Peripherals::take().expect("only taken once");
    kernel::clock::init(p.DCB, p.DWT);
    kernel::time::init(p.SYST);
    kernel::timeout::init();
    // One-shot boot greeting. The host tool's parser resyncs on SOF, so
    // any non-frame bytes that arrive before a session starts are
    // swallowed harmlessly. Once a session is in progress, the kernel
    // emits nothing but framed bytes.
    kernel::println!("monolink ready");

    let cclk_hz = <Mcu as Board>::CCLK_HZ;
    let cycles_per_ms = cclk_hz / 1000;

    let mut parser = Parser::new();
    let mut session: Option<(Loader, ExecHeader)> = None;

    loop {
        match link::poll(&mut parser) {
            Poll::Pending => {
                cortex_m::asm::wfi();
            }
            Poll::Bad(reason) => {
                link::send_nak(framing_reason(reason));
                session = None;
            }
            Poll::Frame(frame) => {
                session = handle_frame(frame, session, cycles_per_ms, cclk_hz);
            }
        }
    }

    fn framing_reason(reason: BadReason) -> NakReason {
        match reason {
            BadReason::Oversized => NakReason::FramingOversized,
            BadReason::CrcMismatch => NakReason::FramingCrcMismatch,
            // `BadSof` is swallowed inside `Parser` between frames and only
            // surfaces when no frame is in progress, so we fall into the
            // generic Unspecified bucket rather than a dedicated reason.
            BadReason::BadSof => NakReason::Unspecified,
        }
    }

    fn loader_reason(err: LoaderError) -> NakReason {
        match err {
            LoaderError::TextTooLarge => NakReason::LoaderTextTooLarge,
            LoaderError::DataTooLarge => NakReason::LoaderDataTooLarge,
            LoaderError::Overflow => NakReason::LoaderOverflow,
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
            _ => {
                link::send_nak(NakReason::UnknownType);
                session
            }
        }
    }

    fn start_load(payload: &[u8]) -> Option<(Loader, ExecHeader)> {
        let Some(header) = ExecHeader::parse(payload) else {
            link::send_nak(NakReason::BadHeaderSize);
            return None;
        };
        if header.magic != ExecHeader::MAGIC || header.version != ExecHeader::VERSION {
            link::send_nak(NakReason::BadMagicOrVersion);
            return None;
        }
        match Loader::new(header.text_size, header.data_size, header.bss_size) {
            Ok(loader) => {
                link::send_ack();
                Some((loader, header))
            }
            Err(e) => {
                link::send_nak(loader_reason(e));
                None
            }
        }
    }

    fn append_chunk(
        payload: &[u8],
        session: Option<(Loader, ExecHeader)>,
    ) -> Option<(Loader, ExecHeader)> {
        let Some((mut loader, header)) = session else {
            link::send_nak(NakReason::ChunkBeforeStart);
            return None;
        };
        match loader.append(payload) {
            Ok(_) => {
                link::send_ack();
                Some((loader, header))
            }
            Err(e) => {
                link::send_nak(loader_reason(e));
                None
            }
        }
    }

    fn finish_load(session: Option<(Loader, ExecHeader)>, cycles_per_ms: u32, cclk_hz: u32) {
        let Some((loader, header)) = session else {
            link::send_nak(NakReason::EofBeforeStart);
            return;
        };
        if loader.written() != loader.expected_total() {
            link::send_nak(NakReason::EofShort);
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
