use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use astra_emu_family_api::{LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioSampleFormat};
use astra_media::{
    DecodeKind, DecodeOutput, DecodeProvider, DecodeRequest, PlayerDecodedAudio,
    SymphoniaAudioDecodeProvider,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

pub(crate) const MAX_RESOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CONVERTED_SAMPLES: usize = 32 * 1024 * 1024;
const TARGET_BUFFERED_FRAMES: usize = 8_192;
const RENDER_CHUNK_FRAMES: usize = 1_024;
const MAX_LOADED_STREAMS: usize = 512;
const MAX_ACTIVE_VOICES: usize = 272;

pub(crate) struct HostAudioExecutor {
    _stream: cpal::Stream,
    producer: NativeAudioProducer,
    telemetry: AudioQueueTelemetryReader,
    sample_rate: u32,
    channels: u16,
    submitted_samples: u64,
    started: bool,
    loaded: BTreeMap<u32, Arc<PlayerDecodedAudio>>,
    streaming_formats: BTreeMap<u32, LegacyAudioSampleFormat>,
    voices: BTreeMap<u32, Voice>,
    master_volume: f32,
    stream_error: Arc<std::sync::atomic::AtomicBool>,
    observed_underflows: u64,
}

struct NativeAudioProducer(Producer<f32>);

struct NativeAudioConsumer {
    inner: Consumer<f32>,
    consumed: Arc<AtomicU64>,
    underflows: Arc<AtomicU64>,
}

#[derive(Clone)]
struct AudioQueueTelemetryReader {
    consumed: Arc<AtomicU64>,
    underflows: Arc<AtomicU64>,
}

impl NativeAudioProducer {
    fn push_samples(&mut self, samples: &[f32]) -> Result<(), ()> {
        if self.0.slots() < samples.len() {
            return Err(());
        }
        for &sample in samples {
            self.0.push(sample).map_err(|_| ())?;
        }
        Ok(())
    }
}

impl NativeAudioConsumer {
    fn pop_sample(&mut self) -> Option<f32> {
        let sample = self.inner.pop().ok()?;
        self.consumed.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }

    fn record_underflow(&self) {
        self.underflows.fetch_add(1, Ordering::Relaxed);
    }
}

impl AudioQueueTelemetryReader {
    fn consumed_samples(&self) -> u64 {
        self.consumed.load(Ordering::Relaxed)
    }

    fn underflows(&self) -> u64 {
        self.underflows.load(Ordering::Relaxed)
    }
}

fn create_queue(
    capacity: usize,
) -> (
    NativeAudioProducer,
    NativeAudioConsumer,
    AudioQueueTelemetryReader,
) {
    let (producer, consumer) = RingBuffer::new(capacity);
    let consumed = Arc::new(AtomicU64::new(0));
    let underflows = Arc::new(AtomicU64::new(0));
    (
        NativeAudioProducer(producer),
        NativeAudioConsumer {
            inner: consumer,
            consumed: Arc::clone(&consumed),
            underflows: Arc::clone(&underflows),
        },
        AudioQueueTelemetryReader {
            consumed,
            underflows,
        },
    )
}

struct Voice {
    audio: Arc<PlayerDecodedAudio>,
    frame_cursor: usize,
    volume: f32,
    pan: f32,
    repeat: bool,
    paused: bool,
    fade: Option<Fade>,
    stopping: bool,
    streaming: bool,
}

struct Fade {
    start: f32,
    target: f32,
    total_frames: u64,
    elapsed_frames: u64,
}

impl HostAudioExecutor {
    pub(crate) fn open() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|_| "ASTRA_EMU_AUDIO_DEFAULT_FORMAT".to_owned())?;
        let config: cpal::StreamConfig = supported.clone().into();
        if config.channels == 0 || config.sample_rate == 0 || config.channels > 8 {
            return Err("ASTRA_EMU_AUDIO_OUTPUT_FORMAT".into());
        }
        let capacity = usize::try_from(config.sample_rate)
            .ok()
            .and_then(|rate| rate.checked_mul(usize::from(config.channels)))
            .and_then(|samples| samples.checked_mul(2))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_QUEUE_BOUNDS".to_owned())?;
        let (producer, consumer, telemetry) = create_queue(capacity);
        let stream_error = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stream = build_stream(
            &device,
            &config,
            supported.sample_format(),
            consumer,
            Arc::clone(&stream_error),
        )?;
        Ok(Self {
            _stream: stream,
            producer,
            telemetry,
            sample_rate: config.sample_rate,
            channels: config.channels,
            submitted_samples: 0,
            started: false,
            loaded: BTreeMap::new(),
            streaming_formats: BTreeMap::new(),
            voices: BTreeMap::new(),
            master_volume: 1.0,
            stream_error,
            observed_underflows: 0,
        })
    }

    #[cfg(target_os = "android")]
    pub(crate) fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        if suspended {
            self._stream
                .pause()
                .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_PAUSE".to_owned())
        } else {
            self._stream
                .play()
                .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_RESUME".to_owned())
        }
    }

    pub(crate) fn execute(
        &mut self,
        command: LegacyAudioCommandV1,
        resolved_resource: Option<Vec<u8>>,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        match command {
            LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding,
                resource_uri,
            } => self.load_resource(
                stream_id,
                encoding,
                &resource_uri,
                resolved_resource.ok_or_else(|| "ASTRA_EMU_AUDIO_RESOURCE_MISSING".to_owned())?,
            ),
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                sample_format,
            } => {
                if sample_rate != self.sample_rate || channels != self.channels {
                    return Err("ASTRA_EMU_AUDIO_STREAM_FORMAT_UNSUPPORTED".into());
                }
                if self.loaded.contains_key(&stream_id) {
                    return Err("ASTRA_EMU_AUDIO_STREAM_DUPLICATE".into());
                }
                if self.loaded.len() >= MAX_LOADED_STREAMS {
                    return Err("ASTRA_EMU_AUDIO_LOADED_STREAM_BUDGET".into());
                }
                self.loaded.insert(
                    stream_id,
                    Arc::new(PlayerDecodedAudio {
                        sample_rate,
                        channels,
                        samples: Vec::new(),
                    }),
                );
                self.streaming_formats.insert(stream_id, sample_format);
                Ok(())
            }
            LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => self.submit_stream(
                stream_id,
                LegacyAudioSampleFormat::I16,
                samples
                    .into_iter()
                    .map(|sample| f32::from(sample) / 32768.0)
                    .collect(),
            ),
            LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => {
                self.submit_stream(stream_id, LegacyAudioSampleFormat::F32, samples)
            }
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
            } => self.play(stream_id, volume, pan, repeat, fade_in_ms),
            LegacyAudioCommandV1::Stop { stream_id, fade_ms } => self.stop(stream_id, fade_ms),
            LegacyAudioCommandV1::Pause { stream_id } => {
                if let Some(voice) = self.voices.get_mut(&stream_id) {
                    voice.paused = true;
                }
                Ok(())
            }
            LegacyAudioCommandV1::Resume { stream_id } => {
                if let Some(voice) = self.voices.get_mut(&stream_id) {
                    voice.paused = false;
                }
                Ok(())
            }
            LegacyAudioCommandV1::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
            } => {
                if let Some(voice) = self.voices.get_mut(&stream_id) {
                    voice.volume = volume;
                    voice.pan = pan;
                    voice.repeat = repeat;
                }
                Ok(())
            }
            LegacyAudioCommandV1::DestroyStream { stream_id } => {
                self.voices.remove(&stream_id);
                self.loaded.remove(&stream_id);
                self.streaming_formats.remove(&stream_id);
                Ok(())
            }
            LegacyAudioCommandV1::MasterVolume { volume } => {
                self.master_volume = volume;
                Ok(())
            }
        }
    }

    pub(crate) fn begin_movie_stream(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        if self.loaded.contains_key(&stream_id) || self.voices.contains_key(&stream_id) {
            return Err("ASTRA_EMU_MOVIE_AUDIO_STREAM_DUPLICATE".into());
        }
        let audio = PlayerDecodedAudio {
            sample_rate,
            channels,
            samples,
        }
        .convert_to(self.sample_rate, self.channels, MAX_CONVERTED_SAMPLES)
        .map_err(|error| error.to_string())?;
        if audio.samples.is_empty() {
            return Err("ASTRA_EMU_MOVIE_AUDIO_FRAME_ALIGNMENT".into());
        }
        self.loaded.insert(stream_id, Arc::new(audio));
        self.streaming_formats
            .insert(stream_id, LegacyAudioSampleFormat::F32);
        self.play(stream_id, 1.0, 0.0, false, 0)
    }

    pub(crate) fn append_movie_stream(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        let audio = PlayerDecodedAudio {
            sample_rate,
            channels,
            samples,
        }
        .convert_to(self.sample_rate, self.channels, MAX_CONVERTED_SAMPLES)
        .map_err(|error| error.to_string())?;
        self.append_streaming_samples(stream_id, audio.samples)
    }

    pub(crate) fn stop_movie_pcm(&mut self, stream_id: u32) -> Result<(), String> {
        self.voices.remove(&stream_id);
        self.streaming_formats.remove(&stream_id);
        self.loaded
            .remove(&stream_id)
            .map(|_| ())
            .ok_or_else(|| "ASTRA_EMU_MOVIE_AUDIO_STREAM_MISSING".to_owned())
    }

    pub(crate) fn pump(&mut self) -> Result<(), String> {
        if self.stream_error.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_DEVICE_LOST".into());
        }
        let underflows = self.telemetry.underflows();
        if !self.started || self.voices.is_empty() {
            // Some native backends invoke one silent callback while the stream is
            // being configured. An exhausted all-silence queue is also not an
            // audible underrun. Establish the baseline before the host fills the
            // queue; only underflows while a voice is active are fatal.
            self.observed_underflows = underflows;
        } else if underflows > self.observed_underflows {
            self.observed_underflows = underflows;
            return Err("ASTRA_EMU_AUDIO_CALLBACK_UNDERFLOW".into());
        }
        let consumed = self.telemetry.consumed_samples();
        let target = TARGET_BUFFERED_FRAMES as u64;
        loop {
            let current_queued =
                self.submitted_samples.saturating_sub(consumed) / u64::from(self.channels);
            if current_queued >= target {
                break;
            }
            let frames = usize::try_from(target - current_queued)
                .unwrap_or(RENDER_CHUNK_FRAMES)
                .min(RENDER_CHUNK_FRAMES);
            let samples = self.mix(frames)?;
            self.producer
                .push_samples(&samples)
                .map_err(|_| "ASTRA_EMU_AUDIO_QUEUE_OVERFLOW".to_owned())?;
            self.submitted_samples = self
                .submitted_samples
                .checked_add(samples.len() as u64)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SAMPLE_COUNTER_OVERFLOW".to_owned())?;
        }
        if !self.started {
            self._stream
                .play()
                .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_START".to_owned())?;
            self.started = true;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), String> {
        if self.started {
            self._stream
                .pause()
                .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_PAUSE".to_owned())?;
        }
        self.started = false;
        self.loaded.clear();
        self.streaming_formats.clear();
        self.voices.clear();
        self.master_volume = 1.0;
        self.observed_underflows = self.telemetry.underflows();
        Ok(())
    }

    fn load_resource(
        &mut self,
        stream_id: u32,
        encoding: LegacyAudioEncoding,
        resource_uri: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        if self.loaded.len() >= MAX_LOADED_STREAMS && !self.loaded.contains_key(&stream_id) {
            return Err("ASTRA_EMU_AUDIO_LOADED_STREAM_BUDGET".into());
        }
        let codec = codec_name(encoding, resource_uri, &bytes)?;
        let decoded = SymphoniaAudioDecodeProvider
            .decode(&DecodeRequest {
                kind: DecodeKind::Audio,
                codec,
                bytes,
                profile: "astra.emu.fvp.audio.v1".into(),
            })
            .map_err(|_| "ASTRA_EMU_AUDIO_DECODE_FAILED".to_owned())?;
        let DecodeOutput::CpuBuffer { bytes, format, .. } = decoded.output else {
            return Err("ASTRA_EMU_AUDIO_DECODE_OUTPUT".into());
        };
        let audio = PlayerDecodedAudio::parse(&format, &bytes, MAX_CONVERTED_SAMPLES)
            .map_err(|error| error.to_string())?
            .convert_to(self.sample_rate, self.channels, MAX_CONVERTED_SAMPLES)
            .map_err(|error| error.to_string())?;
        self.voices.remove(&stream_id);
        self.streaming_formats.remove(&stream_id);
        self.loaded.insert(stream_id, Arc::new(audio));
        Ok(())
    }

    fn submit_stream(
        &mut self,
        stream_id: u32,
        format: LegacyAudioSampleFormat,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        if samples.is_empty() || !samples.len().is_multiple_of(usize::from(self.channels)) {
            return Err("ASTRA_EMU_AUDIO_STREAM_FRAME_ALIGNMENT".into());
        }
        let declared = self
            .streaming_formats
            .get(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_NOT_STREAMING".to_owned())?;
        if *declared != format {
            return Err("ASTRA_EMU_AUDIO_STREAM_SAMPLE_FORMAT_MISMATCH".into());
        }
        let existing = self
            .loaded
            .get(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        let new_len = existing
            .samples
            .len()
            .checked_add(samples.len())
            .filter(|length| *length <= MAX_CONVERTED_SAMPLES)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_SAMPLE_BUDGET".to_owned())?;
        let mut combined = Vec::with_capacity(new_len);
        combined.extend_from_slice(&existing.samples);
        combined.extend_from_slice(&samples);
        let updated = Arc::new(PlayerDecodedAudio {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples: combined,
        });
        self.loaded.insert(stream_id, Arc::clone(&updated));
        if let Some(voice) = self.voices.get_mut(&stream_id) {
            voice.audio = updated;
        }
        Ok(())
    }

    fn play(
        &mut self,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    ) -> Result<(), String> {
        if self.voices.len() >= MAX_ACTIVE_VOICES && !self.voices.contains_key(&stream_id) {
            return Err("ASTRA_EMU_AUDIO_ACTIVE_VOICE_BUDGET".into());
        }
        let audio = self
            .loaded
            .get(&stream_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if audio.samples.is_empty() {
            return Err("ASTRA_EMU_AUDIO_STREAM_EMPTY".into());
        }
        let fade = fade(fade_in_ms, self.sample_rate, 0.0, 1.0)?;
        self.voices.insert(
            stream_id,
            Voice {
                audio,
                frame_cursor: 0,
                volume,
                pan,
                repeat,
                paused: false,
                fade,
                stopping: false,
                streaming: self.streaming_formats.contains_key(&stream_id),
            },
        );
        Ok(())
    }

    fn stop(&mut self, stream_id: u32, fade_ms: u32) -> Result<(), String> {
        let Some(voice) = self.voices.get_mut(&stream_id) else {
            return Ok(());
        };
        if fade_ms == 0 {
            self.voices.remove(&stream_id);
            return Ok(());
        }
        voice.fade = fade(fade_ms, self.sample_rate, 1.0, 0.0)?;
        voice.stopping = true;
        Ok(())
    }

    fn mix(&mut self, frames: usize) -> Result<Vec<f32>, String> {
        let sample_count = frames
            .checked_mul(usize::from(self.channels))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_MIX_BOUNDS".to_owned())?;
        let mut output = vec![0.0_f32; sample_count];
        let mut completed = Vec::new();
        for frame in 0..frames {
            for (&id, voice) in &mut self.voices {
                if voice.paused {
                    continue;
                }
                if voice.frame_cursor >= voice.audio.frame_count() {
                    if voice.streaming {
                        continue;
                    }
                    completed.push(id);
                    continue;
                }
                let source_channels = usize::from(voice.audio.channels);
                let source = voice.frame_cursor * source_channels;
                let fade_gain = voice.fade.as_mut().map_or(1.0, |fade| {
                    let ratio = fade.elapsed_frames as f32 / fade.total_frames as f32;
                    let gain = fade.start + (fade.target - fade.start) * ratio.min(1.0);
                    fade.elapsed_frames = fade.elapsed_frames.saturating_add(1);
                    gain
                });
                let target = frame * usize::from(self.channels);
                for channel in 0..usize::from(self.channels) {
                    let pan_gain = pan_gain(channel, self.channels, voice.pan);
                    output[target + channel] += voice.audio.samples[source + channel]
                        * voice.volume
                        * fade_gain
                        * pan_gain
                        * self.master_volume;
                }
                voice.frame_cursor += 1;
                if voice
                    .fade
                    .as_ref()
                    .is_some_and(|fade| fade.elapsed_frames >= fade.total_frames)
                {
                    voice.fade = None;
                    if voice.stopping {
                        completed.push(id);
                        continue;
                    }
                }
                if voice.frame_cursor == voice.audio.frame_count() {
                    if voice.repeat {
                        voice.frame_cursor = 0;
                    } else {
                        completed.push(id);
                    }
                }
            }
            for id in completed.drain(..) {
                self.voices.remove(&id);
            }
        }
        for sample in &mut output {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(output)
    }
}

impl HostAudioExecutor {
    fn append_streaming_samples(
        &mut self,
        stream_id: u32,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        if samples.is_empty() || !samples.len().is_multiple_of(usize::from(self.channels)) {
            return Err("ASTRA_EMU_AUDIO_STREAM_FRAME_ALIGNMENT".into());
        }
        if self.streaming_formats.get(&stream_id) != Some(&LegacyAudioSampleFormat::F32) {
            return Err("ASTRA_EMU_AUDIO_STREAM_NOT_STREAMING".into());
        }
        let voice = self
            .voices
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_VOICE_MISSING".to_owned())?;
        let consumed_samples = voice
            .frame_cursor
            .checked_mul(usize::from(self.channels))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_SAMPLE_BUDGET".to_owned())?;
        let retained = voice
            .audio
            .samples
            .get(consumed_samples..)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_CURSOR".to_owned())?;
        let new_len = retained
            .len()
            .checked_add(samples.len())
            .filter(|length| *length <= MAX_CONVERTED_SAMPLES)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_SAMPLE_BUDGET".to_owned())?;
        let mut combined = Vec::with_capacity(new_len);
        combined.extend_from_slice(retained);
        combined.extend(samples);
        let updated = Arc::new(PlayerDecodedAudio {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples: combined,
        });
        voice.frame_cursor = 0;
        voice.audio = Arc::clone(&updated);
        self.loaded.insert(stream_id, updated);
        Ok(())
    }
}

fn fade(
    milliseconds: u32,
    sample_rate: u32,
    start: f32,
    target: f32,
) -> Result<Option<Fade>, String> {
    if milliseconds == 0 {
        return Ok(None);
    }
    let total_frames = u64::from(milliseconds)
        .checked_mul(u64::from(sample_rate))
        .map(|frames| frames.div_ceil(1_000))
        .filter(|frames| *frames > 0)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_FADE_BOUNDS".to_owned())?;
    Ok(Some(Fade {
        start,
        target,
        total_frames,
        elapsed_frames: 0,
    }))
}

fn pan_gain(channel: usize, channels: u16, pan: f32) -> f32 {
    if channels != 2 {
        1.0
    } else if channel == 0 {
        if pan > 0.0 {
            1.0 - pan
        } else {
            1.0
        }
    } else if pan < 0.0 {
        1.0 + pan
    } else {
        1.0
    }
}

fn codec_name(encoding: LegacyAudioEncoding, uri: &str, encoded: &[u8]) -> Result<String, String> {
    let declared = match encoding {
        LegacyAudioEncoding::Unknown => None,
        LegacyAudioEncoding::Wav => Some("wav"),
        LegacyAudioEncoding::Ogg => Some("ogg"),
        LegacyAudioEncoding::Mp3 => Some("mp3"),
        LegacyAudioEncoding::Flac => Some("flac"),
    };
    let extension = uri
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    let sniffed = sniff_audio_codec(encoded);
    let codec = declared
        .map(str::to_owned)
        .or(extension)
        .or_else(|| sniffed.map(str::to_owned))
        .ok_or_else(|| "ASTRA_EMU_AUDIO_CODEC_UNKNOWN".to_owned())?;
    if !matches!(codec.as_str(), "wav" | "ogg" | "mp3" | "flac") {
        return Err("ASTRA_EMU_AUDIO_CODEC_UNSUPPORTED".into());
    }
    Ok(codec)
}

fn sniff_audio_codec(encoded: &[u8]) -> Option<&'static str> {
    if encoded.starts_with(b"OggS") {
        Some("ogg")
    } else if encoded.starts_with(b"fLaC") {
        Some("flac")
    } else if encoded.len() >= 12 && encoded.starts_with(b"RIFF") && &encoded[8..12] == b"WAVE" {
        Some("wav")
    } else if encoded.starts_with(b"ID3")
        || matches!(encoded, [0xff, second, ..] if second & 0xe0 == 0xe0)
    {
        Some("mp3")
    } else {
        None
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    consumer: NativeAudioConsumer,
    stream_error: Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, String> {
    match format {
        cpal::SampleFormat::F32 => {
            build_typed_stream::<f32>(device, config, consumer, stream_error, |v| v)
        }
        cpal::SampleFormat::I16 => {
            build_typed_stream::<i16>(device, config, consumer, stream_error, |v| {
                (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
            })
        }
        cpal::SampleFormat::U16 => {
            build_typed_stream::<u16>(device, config, consumer, stream_error, |v| {
                ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX)) as u16
            })
        }
        _ => Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_UNSUPPORTED".into()),
    }
}

fn build_typed_stream<T: cpal::SizedSample + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: NativeAudioConsumer,
    stream_error: Arc<std::sync::atomic::AtomicBool>,
    convert: fn(f32) -> T,
) -> Result<cpal::Stream, String> {
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| {
                let mut underflow = false;
                for sample in output {
                    let value = consumer.pop_sample().unwrap_or_else(|| {
                        underflow = true;
                        0.0
                    });
                    *sample = convert(value);
                }
                if underflow {
                    consumer.record_underflow();
                }
            },
            move |_| {
                stream_error.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_CREATE".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_identity_and_pan_are_strict() {
        assert_eq!(
            codec_name(LegacyAudioEncoding::Unknown, "audio/voice.ogg", b"").unwrap(),
            "ogg"
        );
        assert_eq!(
            codec_name(
                LegacyAudioEncoding::Unknown,
                "voice/resource-without-extension",
                b"OggS\0\0\0\0"
            )
            .unwrap(),
            "ogg"
        );
        assert!(codec_name(LegacyAudioEncoding::Unknown, "audio/voice.exe", b"").is_err());
        assert!(codec_name(
            LegacyAudioEncoding::Unknown,
            "voice/resource-without-extension",
            b"not-audio"
        )
        .is_err());
        assert_eq!(pan_gain(0, 2, -1.0), 1.0);
        assert_eq!(pan_gain(1, 2, -1.0), 0.0);
        assert_eq!(pan_gain(0, 2, 1.0), 0.0);
        assert_eq!(pan_gain(1, 2, 1.0), 1.0);
    }

    #[test]
    fn fade_duration_uses_output_frames_without_rounding_to_zero() {
        let transition = fade(1, 44_100, 0.0, 1.0).unwrap().unwrap();
        assert_eq!(transition.total_frames, 45);
        assert_eq!(transition.elapsed_frames, 0);
        assert!(fade(0, 48_000, 0.0, 1.0).unwrap().is_none());
    }
}
