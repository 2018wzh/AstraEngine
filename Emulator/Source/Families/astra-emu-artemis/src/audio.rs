//! Software audio mixer for the Artemis family.
//!
//! The vendored Artemis core only emits media commands; real decoding and
//! mixing are host-side. This module owns a mixer worker thread that decodes
//! sources with symphonia, applies the Artemis gain/pan/fade/loop semantics,
//! and pushes interleaved stereo i16 chunks into the bounded host sink. A
//! full queue blocks the worker, pacing audio to the real output device;
//! cancellation comes from the sink or the session shutdown.
//!
//! Natural end of a non-looping source is reported back to the session so it
//! can call `notify_sound_finished` on the runtime; the worker never touches
//! the runtime itself.

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use abi_stable::std_types::RVec;
use art3m1s_core::media::MediaSource;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};

use crate::error;

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const CHANNELS: u16 = 2;
pub(crate) const OUTPUT_FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: SAMPLE_RATE,
    channels: CHANNELS,
    format: PcmFormat::I16,
};

/// Stereo frames per sink write: 10 ms at 48 kHz.
const CHUNK_FRAMES: usize = 480;
/// Stereo frames per millisecond at the output rate.
const FRAMES_PER_MS: u64 = 48; // SAMPLE_RATE / 1_000, a literal for const context

// ---------------------------------------------------------------------------
// Mixer protocol
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    Bgm,
    Se,
    Voice,
}

/// A resolved, readable audio source plus the optional A/B loop file.
#[derive(Clone)]
pub(crate) struct SourcePair {
    pub base: Arc<dyn MediaSource>,
    /// Artemis `loop_file`: the segment that loops after the intro pass.
    pub loop_file: Option<Arc<dyn MediaSource>>,
}

pub(crate) enum MixerCommand {
    Play {
        id: Option<String>,
        channel: Channel,
        sources: SourcePair,
        loop_play: bool,
        gain: f32,
        pan: f32,
        fade_ms: u64,
    },
    /// `id: None` targets the single BGM voice.
    Stop {
        id: Option<String>,
        fade_ms: u64,
    },
    Fade {
        id: Option<String>,
        gain: f32,
        time_ms: u64,
    },
    Pan {
        id: Option<String>,
        pan: f32,
        time_ms: u64,
    },
    StopAll {
        fade_ms: u64,
    },
    SetVolume {
        channel: Channel,
        value: f32,
    },
    Shutdown,
}

/// Completion reported from the mixer worker to the session thread.
pub(crate) struct SoundFinished {
    /// `None` is the BGM channel, `Some(id)` an SE or voice.
    pub id: Option<String>,
}

pub(crate) struct AudioBridge {
    tx: Sender<MixerCommand>,
    finished_rx: Receiver<SoundFinished>,
    error: Arc<Mutex<Option<FamilyError>>>,
    cancelled: Arc<AtomicBool>,
    sink: Arc<AudioSinkBox>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioBridge {
    pub(crate) fn start(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT_FORMAT).into_result()?;
        let (tx, rx) = std::sync::mpsc::channel::<MixerCommand>();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel::<SoundFinished>();
        let error = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(sink);
        let worker_state = WorkerState {
            tx: finished_tx,
            error: Arc::clone(&error),
            cancelled: Arc::clone(&cancelled),
        };
        let worker_sink = Arc::clone(&sink);
        let handle = std::thread::Builder::new()
            .name("astra-emu-artemis-audio".into())
            .spawn(move || run_mixer(rx, worker_state, worker_sink))
            .map_err(|_| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_WORKER",
                    "failed to spawn the audio mixer worker",
                )
            })?;
        Ok(Self {
            tx,
            finished_rx,
            error,
            cancelled,
            sink,
            handle: Some(handle),
        })
    }

    /// Sends a command; a dead worker keeps the session alive, the error is
    /// reported by `check_error`.
    pub(crate) fn send(&self, command: MixerCommand) {
        let _ = self.tx.send(command);
    }

    /// Drains natural-completion notifications produced by the worker.
    pub(crate) fn drain_finished(&self) -> Vec<SoundFinished> {
        self.finished_rx.try_iter().collect()
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_AUDIO_CANCELLED",
                "host audio queue is cancelled",
            ));
        }
        if let Some(slot) = self.error.lock().unwrap().as_ref() {
            return Err(slot.clone());
        }
        Ok(())
    }

    /// Cancels the sink first so the worker cannot stay blocked in `write`,
    /// then signals shutdown and joins the worker.
    pub(crate) fn close(mut self) -> FamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.sink.cancel();
        let _ = self.tx.send(MixerCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_WORKER",
                    "the audio mixer worker panicked",
                )
            })?
        }
        if let Some(slot) = self.error.lock().unwrap().as_ref() {
            return Err(slot.clone());
        }
        Ok(())
    }
}

struct WorkerState {
    tx: Sender<SoundFinished>,
    error: Arc<Mutex<Option<FamilyError>>>,
    cancelled: Arc<AtomicBool>,
}

fn report_error(state: &WorkerState, message: String) {
    let mut slot = state.error.lock().unwrap();
    if slot.is_none() {
        *slot = Some(error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_DECODE", message));
    }
}

// ---------------------------------------------------------------------------
// Mixer worker
// ---------------------------------------------------------------------------

struct Voice {
    id: Option<String>,
    channel: Channel,
    loop_play: bool,
    intro_source: Arc<dyn MediaSource>,
    loop_source: Option<Arc<dyn MediaSource>>,
    decoder: Option<SourceReader>,
    resampler: Resampler,
    pending: Vec<[f32; 2]>,
    gain: f32,
    pan: f32,
    fade: Option<Tween>,
    pan_fade: Option<Tween>,
    /// Fading out toward zero; the voice is removed (without a completion
    /// notification) once the fade lands.
    stopping: bool,
}

/// Linear ramp between two values over a frame count.
struct Tween {
    current: f32,
    target: f32,
    remaining_frames: u64,
    total_frames: u64,
}

impl Tween {
    fn start(current: f32, target: f32, time_ms: u64) -> Option<Self> {
        (time_ms > 0).then_some(Self {
            current,
            target,
            remaining_frames: time_ms * FRAMES_PER_MS,
            total_frames: (time_ms * FRAMES_PER_MS).max(1),
        })
    }

    /// Current ramp value with per-frame progression.
    fn step(&mut self) -> f32 {
        let progress = if self.remaining_frames == 0 {
            1.0
        } else {
            1.0 - (self.remaining_frames as f32 / self.total_frames as f32)
        };
        if self.remaining_frames > 0 {
            self.remaining_frames -= 1;
        }
        self.current + (self.target - self.current) * progress
    }

    fn value(&self) -> f32 {
        if self.total_frames == 0 {
            return self.target;
        }
        let progress = 1.0 - (self.remaining_frames as f32 / self.total_frames as f32);
        self.current + (self.target - self.current) * progress
    }
}

/// What the mixer should do with a voice after one chunk.
enum VoiceOutcome {
    Keep,
    Remove,
    RemoveFinished(Option<String>),
}

fn run_mixer(rx: Receiver<MixerCommand>, state: WorkerState, sink: Arc<AudioSinkBox>) {
    let mut voices: Vec<Voice> = Vec::new();
    let mut volumes = [1.0_f32; 3]; // bgm, se, voice
    let master = 1.0_f32;
    let mut mix = vec![0.0_f32; CHUNK_FRAMES * 2];
    // Real-time pacing: with a sink that accepts instantly (a test sink or a
    // fast consumer), a looping source would otherwise decode at full CPU
    // speed. The mixer stays a bounded lookahead ahead of the wall clock;
    // the bounded sink queue keeps this exact pacing on the real device.
    let started = std::time::Instant::now();
    let mut produced_frames: u64 = 0;

    loop {
        // At least one blocking recv when nothing can be mixed; otherwise
        // drain everything pending without blocking.
        let mut commands = Vec::new();
        if voices.is_empty() {
            match rx.recv() {
                Ok(command) => commands.push(command),
                Err(_) => return,
            }
        }
        while let Ok(command) = rx.try_recv() {
            commands.push(command);
        }
        for command in commands {
            match command {
                MixerCommand::Shutdown => return,
                MixerCommand::Play {
                    id,
                    channel,
                    sources,
                    loop_play,
                    gain,
                    pan,
                    fade_ms,
                } => {
                    // Same-id replacement and BGM single-channel semantics.
                    voices.retain(|voice| !same_target(voice, &id, channel));
                    let opened = SourceReader::open(Arc::clone(&sources.base));
                    if let Err(open_error) = &opened {
                        report_error(&state, format!("open audio source: {open_error}"));
                    }
                    let source_rate = opened.as_ref().ok().map(|decoder| decoder.sample_rate);
                    voices.push(Voice {
                        id,
                        channel,
                        loop_play,
                        intro_source: sources.base,
                        loop_source: sources.loop_file,
                        decoder: opened.ok(),
                        resampler: Resampler::new(source_rate),
                        pending: Vec::new(),
                        gain,
                        pan,
                        fade: Tween::start(0.0, gain, fade_ms),
                        pan_fade: None,
                        stopping: false,
                    });
                }
                MixerCommand::Stop { id, fade_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|voice| same_target(voice, &id, voice.channel))
                    {
                        voice.stopping = true;
                        if fade_ms == 0 {
                            voice.fade = None;
                            voice.pending.clear();
                        } else {
                            let current =
                                voice.fade.as_ref().map_or(1.0, Tween::value) * voice.gain;
                            voice.fade = Tween::start(current, 0.0, fade_ms);
                        }
                    }
                }
                MixerCommand::Fade { id, gain, time_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|voice| same_target(voice, &id, voice.channel))
                    {
                        let current = voice.fade.as_ref().map_or(1.0, Tween::value) * voice.gain;
                        voice.gain = gain;
                        voice.fade = Tween::start(current, gain, time_ms);
                    }
                }
                MixerCommand::Pan { id, pan, time_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|voice| same_target(voice, &id, voice.channel))
                    {
                        if time_ms == 0 {
                            voice.pan_fade = None;
                            voice.pan = pan;
                        } else {
                            voice.pan_fade = Some(Tween {
                                current: voice.pan,
                                target: pan,
                                remaining_frames: time_ms * FRAMES_PER_MS,
                                total_frames: (time_ms * FRAMES_PER_MS).max(1),
                            });
                        }
                    }
                }
                MixerCommand::StopAll { fade_ms } => {
                    for voice in voices.iter_mut() {
                        voice.stopping = true;
                        if fade_ms == 0 {
                            voice.fade = None;
                            voice.pending.clear();
                        } else {
                            let current =
                                voice.fade.as_ref().map_or(1.0, Tween::value) * voice.gain;
                            voice.fade = Tween::start(current, 0.0, fade_ms);
                        }
                    }
                }
                MixerCommand::SetVolume { channel, value } => match channel {
                    Channel::Bgm => volumes[0] = value,
                    Channel::Se => volumes[1] = value,
                    Channel::Voice => volumes[2] = value,
                },
            }
        }

        if voices.is_empty() {
            continue;
        }

        mix.fill(0.0);
        let channel_factors = [
            (master * volumes[0]).clamp(0.0, 1.0),
            (master * volumes[1]).clamp(0.0, 1.0),
            (master * volumes[2]).clamp(0.0, 1.0),
        ];
        voices.retain_mut(|voice| {
            let factor = match voice.channel {
                Channel::Bgm => channel_factors[0],
                Channel::Se => channel_factors[1],
                Channel::Voice => channel_factors[2],
            };
            match mix_voice(voice, mix.as_mut_slice(), factor) {
                VoiceOutcome::Keep => true,
                VoiceOutcome::Remove => false,
                VoiceOutcome::RemoveFinished(id) => {
                    let _ = state.tx.send(SoundFinished { id });
                    false
                }
            }
        });

        let mut chunk = RVec::with_capacity(CHUNK_FRAMES * 2);
        for sample in &mix {
            chunk.push((sample.clamp(-1.0, 1.0) * 32_767.0) as i16);
        }
        if state.cancelled.load(Ordering::Acquire) {
            return;
        }
        produced_frames = produced_frames.saturating_add(CHUNK_FRAMES as u64);
        let lookahead_frames = u64::from(SAMPLE_RATE) / 5; // 200 ms
        let pace_budget = (started.elapsed().as_secs_f64() * f64::from(SAMPLE_RATE)) as u64 * 105
            / 100
            + lookahead_frames;
        if produced_frames > pace_budget {
            let ahead_secs = (produced_frames - pace_budget) as f64 / f64::from(SAMPLE_RATE);
            std::thread::sleep(std::time::Duration::from_secs_f64(ahead_secs.min(0.05)));
        }
        match sink.write(PcmChunk::I16(chunk)).into_result() {
            Ok(AudioWriteStatus::Accepted) => {}
            Ok(AudioWriteStatus::Cancelled) | Ok(AudioWriteStatus::Closed) => return,
            Err(write_error) => {
                report_error(&state, format!("host sink write failed: {write_error}"));
                return;
            }
        }
    }
}

fn same_target(voice: &Voice, id: &Option<String>, channel: Channel) -> bool {
    voice.channel == channel
        && match (&voice.id, id) {
            (None, None) => true,
            (Some(voice_id), Some(id)) => voice_id == id,
            _ => false,
        }
}

/// Mixes one voice into the chunk buffer.
fn mix_voice(voice: &mut Voice, mix: &mut [f32], channel_factor: f32) -> VoiceOutcome {
    // Refill pending ahead of the mix window; a bounded budget keeps a huge
    // decode burst from stalling other voices.
    let mut fill_budget = CHUNK_FRAMES * 8;
    while voice.pending.len() < CHUNK_FRAMES && fill_budget > 0 {
        let Some(decoder) = voice.decoder.as_mut() else {
            break;
        };
        let mut decoded = Vec::new();
        match decoder.next_stereo(&mut decoded) {
            Ok(true) => {
                voice.resampler.push(&decoded, &mut voice.pending);
                fill_budget = fill_budget.saturating_sub(decoded.len().max(1));
            }
            Ok(false) => {
                if voice.stopping {
                    return VoiceOutcome::Remove;
                }
                if voice.loop_play {
                    // A/B loop: after the intro pass, loop the loop segment
                    // (or the base source when no loop file exists).
                    let source = voice
                        .loop_source
                        .clone()
                        .unwrap_or_else(|| Arc::clone(&voice.intro_source));
                    let reopened = SourceReader::open(source);
                    if let Err(open_error) = &reopened {
                        tracing::warn!(
                            event = "astra.emu.artemis.audio.loop_reopen_failed",
                            detail = %open_error
                        );
                        return VoiceOutcome::RemoveFinished(voice.id.clone());
                    }
                    voice.decoder = reopened.ok();
                    voice.resampler =
                        Resampler::new(voice.decoder.as_ref().map(|decoder| decoder.sample_rate));
                    voice.pending.clear();
                    fill_budget = fill_budget.saturating_sub(1);
                } else {
                    return VoiceOutcome::RemoveFinished(voice.id.clone());
                }
            }
            Err(decode_error) => {
                tracing::warn!(
                    event = "astra.emu.artemis.audio.decode_failed",
                    detail = %decode_error
                );
                return VoiceOutcome::RemoveFinished(voice.id.clone());
            }
        }
    }

    let fill_len = voice.pending.len().min(CHUNK_FRAMES);
    for index in 0..CHUNK_FRAMES {
        let frame = if index < fill_len {
            voice.pending[index]
        } else {
            [0.0, 0.0]
        };
        let fade_factor = voice.fade.as_mut().map_or(1.0, Tween::step);
        let pan = match &mut voice.pan_fade {
            Some(tween) => tween.step(),
            None => voice.pan,
        };
        let gain = (channel_factor * voice.gain * fade_factor).clamp(0.0, 1.0);
        let left = gain * (1.0 - pan.max(0.0));
        let right = gain * (1.0 + pan.min(0.0));
        let offset = index * 2;
        mix[offset] += frame[0] * left;
        mix[offset + 1] += frame[1] * right;
    }
    voice.pending.drain(..fill_len);

    if voice.stopping {
        return match &voice.fade {
            Some(fade) if fade.remaining_frames > 0 && fade.target > 0.0 => VoiceOutcome::Keep,
            _ => VoiceOutcome::Remove,
        };
    }
    VoiceOutcome::Keep
}

// ---------------------------------------------------------------------------
// symphonia streaming decoder over the random-access host source
// ---------------------------------------------------------------------------

/// Bridges the random-access `art3m1s_media::MediaSource` into `Read + Seek`
/// for symphonia's `MediaSourceStream`.
struct SourceReaderAdapter {
    source: Arc<dyn MediaSource>,
    position: u64,
    length: u64,
}

impl Read for SourceReaderAdapter {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.length.saturating_sub(self.position);
        let request = buf
            .len()
            .min(usize::try_from(remaining).unwrap_or(buf.len()));
        if request == 0 {
            return Ok(0);
        }
        let read = self
            .source
            .read_at(self.position, &mut buf[..request])
            .map_err(std::io::Error::other)?;
        self.position += read as u64;
        Ok(read)
    }
}

impl symphonia::core::io::MediaSource for SourceReaderAdapter {
    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }

    fn is_seekable(&self) -> bool {
        true
    }
}

impl Seek for SourceReaderAdapter {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let target = match position {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => i64::try_from(self.length)
                .ok()
                .and_then(|length| length.checked_add(offset))
                .and_then(|value| u64::try_from(value).ok()),
            SeekFrom::Current(offset) => i64::try_from(self.position)
                .ok()
                .and_then(|current| current.checked_add(offset))
                .and_then(|value| u64::try_from(value).ok()),
        };
        match target {
            Some(offset) if offset <= self.length => {
                self.position = offset;
                Ok(self.position)
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek target is outside the media source",
            )),
        }
    }
}

/// Streams one audio source into stereo f32 frames at the source rate.
struct SourceReader {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    sample_buffer: Option<symphonia::core::audio::SampleBuffer<f32>>,
}

impl SourceReader {
    fn open(source: Arc<dyn MediaSource>) -> Result<Self, String> {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let length = source.len().map_err(|len_error| len_error.to_string())?;
        let reader = SourceReaderAdapter {
            source,
            position: 0,
            length,
        };
        let mss = MediaSourceStream::new(Box::new(reader), Default::default());
        let hint = Hint::new();
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions {
                    enable_gapless: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|probe_error| format!("probe audio stream: {probe_error}"))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or("audio stream has no decodable track")?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(SAMPLE_RATE);
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|decoder_error| format!("create audio decoder: {decoder_error}"))?;
        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            sample_buffer: None,
        })
    }

    /// Decodes forward until at least one stereo frame is produced. Returns
    /// `false` on end of stream.
    fn next_stereo(&mut self, out: &mut Vec<[f32; 2]>) -> Result<bool, String> {
        use symphonia::core::errors::Error as SymphoniaError;
        out.clear();
        while out.is_empty() {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(false)
                }
                Err(SymphoniaError::ResetRequired) => return Ok(false),
                Err(error) => return Err(format!("read audio packet: {error}")),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(error) => return Err(format!("decode audio packet: {error}")),
            };
            if decoded.frames() == 0 {
                continue;
            }
            let spec = *decoded.spec();
            let frames = decoded.frames().max(decoded.capacity()) as u64;
            let buffer = self.sample_buffer.get_or_insert_with(|| {
                symphonia::core::audio::SampleBuffer::<f32>::new(frames, spec)
            });
            if buffer.capacity() < decoded.frames() {
                *buffer = symphonia::core::audio::SampleBuffer::<f32>::new(frames, spec);
            }
            buffer.copy_interleaved_ref(decoded);
            let channels = spec.channels.count().max(1);
            for frame in buffer.samples().chunks(channels) {
                let left = frame.first().copied().unwrap_or(0.0);
                let right = frame.get(1).copied().unwrap_or(left);
                out.push([left, right]);
            }
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Linear resampler to the fixed output rate
// ---------------------------------------------------------------------------

struct Resampler {
    step: f64,
    position: f64,
    previous: [f32; 2],
    has_previous: bool,
}

impl Resampler {
    fn new(source_rate: Option<u32>) -> Self {
        Self {
            step: source_rate.map_or(1.0, |rate| f64::from(rate) / f64::from(SAMPLE_RATE)),
            position: 0.0,
            previous: [0.0; 2],
            has_previous: false,
        }
    }

    fn push(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) {
        if input.is_empty() {
            return;
        }
        if (self.step - 1.0).abs() < 1e-9 {
            output.extend_from_slice(input);
            self.previous = *input.last().unwrap();
            self.has_previous = true;
            self.position = 0.0;
            return;
        }
        loop {
            let index = self.position as usize;
            if index >= input.len() {
                break;
            }
            let fraction = self.position - index as f64;
            let a = if index == 0 {
                if self.has_previous {
                    self.previous
                } else {
                    input[0]
                }
            } else {
                input[index - 1]
            };
            let b = input[index];
            output.push([
                a[0] + (b[0] - a[0]) * fraction as f32,
                a[1] + (b[1] - a[1]) * fraction as f32,
            ]);
            self.position += self.step;
        }
        let consumed = (self.position as usize).min(input.len());
        if consumed > 0 {
            self.previous = input[consumed - 1];
            self.has_previous = true;
            self.position -= consumed as f64;
        }
    }
}
