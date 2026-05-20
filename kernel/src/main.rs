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
        self, BadReason, CHUNK_MAX, ExecHeader, NakReason, Parser, Poll, ResultFrame, TYPE_CASE,
        TYPE_CASE_END, TYPE_CASE_OUTPUT, TYPE_CHUNK, TYPE_EOF, TYPE_RUN_END, TYPE_START_LOAD,
    };
    use kernel::user::{self, AbortStatus, LoadedProgram, Loader, LoaderError};

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

    enum Stage {
        /// Waiting for a `START_LOAD` to begin a new upload.
        Idle,
        /// Receiving `CHUNK` frames; transitions to `Running` on `EOF`.
        Loading { loader: Loader, header: ExecHeader },
        /// Program loaded; cases run one at a time on `CASE_END`.
        Running { header: ExecHeader },
    }

    let mut parser = Parser::new();
    let mut stage = Stage::Idle;

    loop {
        let frame = match link::poll(&mut parser) {
            Poll::Pending => {
                cortex_m::asm::wfi();
                continue;
            }
            Poll::Bad(reason) => {
                link::send_nak(framing_reason(reason));
                stage = Stage::Idle;
                continue;
            }
            Poll::Frame(f) => f,
        };

        stage = match (frame.kind, stage) {
            // ---- upload phase ----
            (TYPE_START_LOAD, _) => start_load(frame.payload),
            (TYPE_CHUNK, Stage::Loading { loader, header }) => {
                append_chunk(frame.payload, loader, header)
            }
            (TYPE_EOF, Stage::Loading { loader, header }) => finish_load(loader, header),
            (TYPE_CHUNK | TYPE_EOF, _) => {
                link::send_nak(NakReason::ChunkBeforeStart);
                Stage::Idle
            }
            // ---- run phase ----
            (TYPE_CASE, Stage::Running { header }) => {
                match user::CASE_IO.append_input(frame.payload) {
                    Ok(()) => link::send_ack(),
                    Err(_) => link::send_nak(NakReason::CaseInputOverflow),
                }
                Stage::Running { header }
            }
            (TYPE_CASE_END, Stage::Running { header }) => {
                run_one_case(&header, cycles_per_ms, cclk_hz)
            }
            (TYPE_RUN_END, Stage::Running { .. }) => {
                send_final_result(link::STATUS_OK, cclk_hz);
                Stage::Idle
            }
            (TYPE_CASE | TYPE_CASE_END, _) => {
                link::send_nak(NakReason::CaseBeforeRun);
                Stage::Idle
            }
            (TYPE_RUN_END, _) => {
                link::send_nak(NakReason::RunEndBeforeRun);
                Stage::Idle
            }
            _ => {
                link::send_nak(NakReason::UnknownType);
                Stage::Idle
            }
        };
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

    fn abort_status_byte(status: AbortStatus) -> u8 {
        match status {
            AbortStatus::Ok => link::STATUS_OK,
            AbortStatus::Timeout => link::STATUS_TIMEOUT,
            AbortStatus::MemFault => link::STATUS_MEMFAULT,
            AbortStatus::BusFault => link::STATUS_BUSFAULT,
            AbortStatus::UsageFault => link::STATUS_USAGEFAULT,
            AbortStatus::LoadError => link::STATUS_LOAD_ERROR,
        }
    }

    fn start_load(payload: &[u8]) -> Stage {
        let Some(header) = ExecHeader::parse(payload) else {
            link::send_nak(NakReason::BadHeaderSize);
            return Stage::Idle;
        };
        if header.magic != ExecHeader::MAGIC || header.version != ExecHeader::VERSION {
            link::send_nak(NakReason::BadMagicOrVersion);
            return Stage::Idle;
        }
        match Loader::new(header.text_size, header.data_size, header.bss_size) {
            Ok(loader) => {
                link::send_ack();
                Stage::Loading { loader, header }
            }
            Err(e) => {
                link::send_nak(loader_reason(e));
                Stage::Idle
            }
        }
    }

    fn append_chunk(payload: &[u8], mut loader: Loader, header: ExecHeader) -> Stage {
        match loader.append(payload) {
            Ok(_) => {
                link::send_ack();
                Stage::Loading { loader, header }
            }
            Err(e) => {
                link::send_nak(loader_reason(e));
                Stage::Idle
            }
        }
    }

    fn finish_load(loader: Loader, header: ExecHeader) -> Stage {
        if loader.written() != loader.expected_total() {
            link::send_nak(NakReason::EofShort);
            return Stage::Idle;
        }
        // Snapshot the `.data` init image into kernel SRAM so we can
        // restore it between cases.
        user::DATA_INIT_IMAGE.capture(header.data_size, header.bss_size);
        user::CASE_IO.reset_input();
        link::send_ack();
        Stage::Running { header }
    }

    fn run_one_case(header: &ExecHeader, cycles_per_ms: u32, cclk_hz: u32) -> Stage {
        // Restore fresh `.data`/`.bss`; reset I/O cursors for this run.
        user::DATA_INIT_IMAGE.restore_to_user_data();
        user::CASE_IO.prepare_for_launch();

        let res = user::launch_loaded(LoadedProgram {
            entry_offset: header.entry_offset,
            text_size: header.text_size,
            data_size: header.data_size,
            bss_size: header.bss_size,
            timeout_cycles: header.timeout_ms.saturating_mul(cycles_per_ms),
        });

        // Stream any output the user produced, then close with a result.
        for chunk in user::CASE_IO.output().chunks(CHUNK_MAX) {
            link::send(TYPE_CASE_OUTPUT, chunk);
        }
        let status = abort_status_byte(res.status);
        link::send_case_result(status, res.payload, res.cycles);

        user::CASE_IO.reset_input();

        if res.status == AbortStatus::Ok {
            Stage::Running { header: *header }
        } else {
            send_final_result(status, cclk_hz);
            Stage::Idle
        }
    }

    fn send_final_result(status: u8, cclk_hz: u32) {
        let mut bytes = [0u8; ResultFrame::SIZE];
        ResultFrame {
            status,
            _pad: [0; 3],
            exit_code: 0,
            cycles: 0,
            cclk_hz,
        }
        .write_le(&mut bytes);
        link::send(link::TYPE_RESULT, &bytes);
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
