//! Transport fixture only. No VM, GPU, native boot or input implementation.
use astra_emu_krkr::{
    child::{self, AudioCallbacks, NativeOwner},
    wire::{self, Failure, FrameInfo, Reply, WindowState},
};

struct TransportFixture {
    audio: Option<AudioCallbacks>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl NativeOwner for TransportFixture {
    fn boot(
        &mut self,
        _: &str,
        _: WindowState,
        audio: AudioCallbacks,
    ) -> Result<FrameInfo, Failure> {
        if std::env::var("KRKR_TRANSPORT_FIXTURE").as_deref() == Ok("pcm-once") {
            if audio
                .write(&[7; wire::PCM_SAMPLES])
                .map_err(|_| Failure::Audio)?
                != wire::WriteStatus::Accepted
            {
                return Err(Failure::Audio);
            }
            if audio.progress()
                != Some(wire::Progress {
                    accepted_source_frames: 480,
                    played_source_frames: 480,
                    clock: wire::PlaybackClock::TestConsumption,
                })
            {
                return Err(Failure::Audio);
            }
        }
        if std::env::var("KRKR_TRANSPORT_FIXTURE").as_deref() == Ok("pcm-wait") {
            let callback = audio.clone();
            self.worker = Some(std::thread::spawn(move || {
                let _ = callback.write(&[0; wire::PCM_SAMPLES]);
            }));
            self.audio = Some(audio);
            // Wait for test parent cancellation. This is an injected
            // transport wait, not a working native boot.
            while !self.audio.as_ref().unwrap().is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Err(Failure::NativeUnavailable)
    }
    fn advance(&mut self, _: u64, _: &[wire::Event]) -> Result<Reply, Failure> {
        Err(Failure::NativeUnavailable)
    }
    fn capture(&mut self) -> Result<(FrameInfo, Vec<u8>), Failure> {
        Err(Failure::NativeUnavailable)
    }
    fn shutdown(&mut self) -> Result<(), Failure> {
        if let Some(audio) = self.audio.take() {
            audio.cancel();
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| Failure::Shutdown)?;
        }
        Ok(())
    }
}

fn main() {
    let mode = std::env::var("KRKR_TRANSPORT_FIXTURE").unwrap_or_default();
    if mode == "early-exit" {
        return;
    }
    if mode == "stdio-noise" {
        println!("KrKr transport fixture stdout diagnostics, never a frame");
        eprintln!("KrKr transport fixture stderr diagnostics, never protocol");
    }
    let result = child::connection_arguments().and_then(|(session, port)| {
        if matches!(
            mode.as_str(),
            "ignore-stop"
                | "stale-control"
                | "partial-control"
                | "unknown-control"
                | "blocked-frame"
        ) {
            return child::transport_fixture(&mode, session, port);
        }
        child::serve(
            session,
            port,
            TransportFixture {
                audio: None,
                worker: None,
            },
        )
    });
    if result.is_err() {
        std::process::exit(1);
    }
}
