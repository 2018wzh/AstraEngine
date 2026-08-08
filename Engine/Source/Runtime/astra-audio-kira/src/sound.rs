use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
};

use kira::{
    sound::{Sound, SoundData},
    Frame,
};

const PLAYING: u8 = 0;
const PAUSED: u8 = 1;
const STOPPED: u8 = 2;
const COMPLETED: u8 = 3;
const NO_SEEK: u64 = u64::MAX;

#[derive(Debug)]
struct SharedPlayback {
    state: AtomicU8,
    cursor_frames: AtomicU64,
    seek_frame: AtomicU64,
}

/// Device-format PCM prepared outside the render callback. Cloning this value shares the
/// decoder-owned sample allocation; `Sound::process` only reads it.
#[derive(Debug, Clone)]
pub struct AstraPcmSoundData {
    samples: Arc<Vec<f32>>,
    channels: u16,
    start_frame: u64,
    looping: bool,
}

impl AstraPcmSoundData {
    pub fn new(
        samples: Arc<Vec<f32>>,
        channels: u16,
        start_frame: u64,
        looping: bool,
    ) -> Result<Self, &'static str> {
        if !matches!(channels, 1 | 2)
            || samples.is_empty()
            || !samples.len().is_multiple_of(usize::from(channels))
            || start_frame >= (samples.len() / usize::from(channels)) as u64
        {
            return Err("PCM sound shape or start frame is invalid");
        }
        Ok(Self {
            samples,
            channels,
            start_frame,
            looping,
        })
    }

    #[must_use]
    pub fn allocation_ptr(&self) -> *const f32 {
        self.samples.as_ptr()
    }
}

#[derive(Debug, Clone)]
pub struct AstraPcmSoundHandle {
    shared: Arc<SharedPlayback>,
}

impl AstraPcmSoundHandle {
    pub fn pause(&self) {
        if self.shared.state.load(Ordering::Acquire) == PLAYING {
            self.shared.state.store(PAUSED, Ordering::Release);
        }
    }

    pub fn resume(&self) {
        if self.shared.state.load(Ordering::Acquire) == PAUSED {
            self.shared.state.store(PLAYING, Ordering::Release);
        }
    }

    pub fn stop(&self) {
        self.shared.state.store(STOPPED, Ordering::Release);
    }

    pub fn seek(&self, frame: u64) {
        self.shared.seek_frame.store(frame, Ordering::Release);
    }

    #[must_use]
    pub fn cursor_frames(&self) -> u64 {
        self.shared.cursor_frames.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.shared.state.load(Ordering::Acquire) == COMPLETED
    }
}

impl SoundData for AstraPcmSoundData {
    type Error = Infallible;
    type Handle = AstraPcmSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let shared = Arc::new(SharedPlayback {
            state: AtomicU8::new(PLAYING),
            cursor_frames: AtomicU64::new(self.start_frame),
            seek_frame: AtomicU64::new(NO_SEEK),
        });
        let handle = AstraPcmSoundHandle {
            shared: Arc::clone(&shared),
        };
        Ok((
            Box::new(AstraPcmSound {
                samples: self.samples,
                channels: self.channels,
                cursor: self.start_frame,
                looping: self.looping,
                shared,
            }),
            handle,
        ))
    }
}

struct AstraPcmSound {
    samples: Arc<Vec<f32>>,
    channels: u16,
    cursor: u64,
    looping: bool,
    shared: Arc<SharedPlayback>,
}

impl AstraPcmSound {
    fn frame_count(&self) -> u64 {
        (self.samples.len() / usize::from(self.channels)) as u64
    }
}

impl Sound for AstraPcmSound {
    fn on_start_processing(&mut self) {
        let seek = self.shared.seek_frame.swap(NO_SEEK, Ordering::AcqRel);
        if seek != NO_SEEK {
            if seek < self.frame_count() {
                self.cursor = seek;
                self.shared.cursor_frames.store(seek, Ordering::Release);
            } else {
                self.shared.state.store(STOPPED, Ordering::Release);
            }
        }
    }

    fn process(&mut self, out: &mut [Frame], _dt: f64, _info: &kira::info::Info) {
        if self.shared.state.load(Ordering::Acquire) != PLAYING {
            out.fill(Frame::ZERO);
            return;
        }
        let frame_count = self.frame_count();
        for output in out {
            if self.cursor == frame_count {
                if self.looping {
                    self.cursor = 0;
                } else {
                    self.shared.state.store(COMPLETED, Ordering::Release);
                    *output = Frame::ZERO;
                    continue;
                }
            }
            let offset = self.cursor as usize * usize::from(self.channels);
            *output = if self.channels == 1 {
                Frame::from_mono(self.samples[offset])
            } else {
                Frame::new(self.samples[offset], self.samples[offset + 1])
            };
            self.cursor += 1;
        }
        self.shared
            .cursor_frames
            .store(self.cursor, Ordering::Release);
    }

    fn finished(&self) -> bool {
        matches!(
            self.shared.state.load(Ordering::Acquire),
            STOPPED | COMPLETED
        )
    }
}
