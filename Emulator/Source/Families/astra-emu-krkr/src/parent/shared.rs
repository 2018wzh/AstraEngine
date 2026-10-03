use crate::wire::{Failure, PlaybackClock, Progress};
use astra_emu_family_api::{AudioPlaybackClock, AudioSinkBox, PcmFormat, PcmFormatSpec};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub(super) struct Shared {
    pub sink: Arc<AudioSinkBox>,
    pub io_stop: Arc<AtomicBool>,
    pub pcm_stop: AtomicBool,
    pub closing: AtomicBool,
    pub active: AtomicBool,
    pub cancel_requested: AtomicBool,
    pub progress_valid: AtomicBool,
    progress: Mutex<Progress>,
    first: Mutex<Option<Failure>>,
}

pub(super) fn failure_error(failure: Failure) -> io::Error {
    io::Error::new(
        if failure == Failure::NativeUnavailable {
            io::ErrorKind::Unsupported
        } else {
            io::ErrorKind::Other
        },
        failure,
    )
}

fn query(sink: &AudioSinkBox) -> io::Result<Progress> {
    let p = sink.playback_progress().into_result().map_err(|error| {
        if error.code() == "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED" {
            io::Error::new(
                io::ErrorKind::Unsupported,
                astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED",
                    "host audio sink does not report actual playback progress",
                ),
            )
        } else {
            failure_error(Failure::Audio)
        }
    })?;
    let p = Progress {
        accepted_source_frames: p.accepted_source_frames,
        played_source_frames: p.played_source_frames,
        clock: match p.clock {
            AudioPlaybackClock::DeviceTimestamp => PlaybackClock::DeviceTimestamp,
            AudioPlaybackClock::TestConsumption => PlaybackClock::TestConsumption,
        },
    };
    p.validate().map_err(|_| failure_error(Failure::Audio))?;
    Ok(p)
}

impl Shared {
    pub fn configure(sink: Arc<AudioSinkBox>) -> io::Result<Arc<Self>> {
        let result = (|| {
            sink.configure(PcmFormatSpec {
                sample_rate: 48_000,
                channels: 2,
                format: PcmFormat::I16,
            })
            .into_result()
            .map_err(|_| failure_error(Failure::Audio))?;
            let progress = query(&sink)?;
            if progress.accepted_source_frames != 0 || progress.played_source_frames != 0 {
                return Err(failure_error(Failure::Audio));
            }
            Ok(progress)
        })();
        let progress = match result {
            Ok(progress) => progress,
            Err(first) => {
                let _ = sink.cancel().into_result();
                return Err(first);
            }
        };
        Ok(Arc::new(Self {
            sink,
            io_stop: Arc::new(AtomicBool::new(false)),
            pcm_stop: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            active: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            progress_valid: AtomicBool::new(true),
            progress: Mutex::new(progress),
            first: Mutex::new(None),
        }))
    }

    pub fn initial_progress(&self) -> Progress {
        *self.progress.lock().unwrap()
    }

    /// Only queries the real Host sink. Queries are serialized so concurrent
    /// PCM acknowledgements and polling cannot manufacture a regression.
    pub fn observe(&self) -> io::Result<Progress> {
        if !self.progress_valid.load(Ordering::Acquire)
            || self.pcm_stop.load(Ordering::Acquire)
            || self.sink.is_cancelled()
        {
            return Err(failure_error(Failure::Audio));
        }
        let mut previous = self.progress.lock().unwrap();
        let progress = query(&self.sink)?;
        progress
            .validate_after(*previous)
            .map_err(|_| failure_error(Failure::Audio))?;
        *previous = progress;
        Ok(progress)
    }

    pub fn first(&self) -> Option<Failure> {
        *self.first.lock().unwrap()
    }
    pub fn fail(&self, failure: Failure) {
        let mut first = self.first.lock().unwrap();
        if first.is_none() {
            *first = Some(failure);
        }
        drop(first);
        self.invalidate();
        // cancel does not join and must release a blocked write.
        let _ = self.sink.cancel().into_result();
    }
    pub fn invalidate(&self) {
        self.progress_valid.store(false, Ordering::Release);
        self.pcm_stop.store(true, Ordering::Release);
    }
    pub fn cancel(&self) {
        self.invalidate();
        if self.sink.cancel().into_result().is_err() {
            self.fail(Failure::Audio);
        }
    }
}
