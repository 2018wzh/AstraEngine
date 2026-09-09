use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[path = "audio_conversion.rs"]
mod audio_conversion;

use abi_stable::type_level::downcasting::TD_Opaque;
use astra_emu_family_api::{
    AudioSink, AudioSinkBox, AudioSink_TO, AudioWriteStatus, PcmChunk, PcmFormat, PcmFormatSpec,
    MAX_AUDIO_SAMPLES_PER_CHUNK,
};
#[cfg(test)]
use audio_conversion::{convert_chunk, convert_samples};
use audio_conversion::{pcm_chunk_samples, AudioConverter, OutputFormat};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, select, Receiver, Sender};

const AUDIO_QUEUE_CHUNKS: usize = 16;
const MAX_AUDIO_OUTPUT_SAMPLES: usize = MAX_AUDIO_SAMPLES_PER_CHUNK;

/// Host-owned audio output. The family only sees the ABI sink; it never sees
/// CPAL, the device callback, or a native handle. `configure` is lazy because
/// the family declares its source format during open.
pub(crate) struct HostAudioExecutor {
    state: Arc<AudioState>,
}

struct AudioState {
    cancelled: AtomicBool,
    closed: AtomicBool,
    stream_failed: AtomicBool,
    configured: Mutex<Option<OutputFormat>>,
    producer: Mutex<Option<Sender<Vec<f32>>>>,
    cancel_waiters: Mutex<Vec<Sender<()>>>,
    stream: Mutex<Option<cpal::Stream>>,
    converter: Mutex<Option<AudioConverter>>,
}

type CancelWaiter = (Sender<()>, Receiver<()>);

struct HostAudioSink {
    state: Arc<AudioState>,
}

struct DeviceConsumer {
    receiver: Receiver<Vec<f32>>,
    blocks: VecDeque<Vec<f32>>,
    current: Option<Vec<f32>>,
    cursor: usize,
}

impl HostAudioExecutor {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(AudioState {
                cancelled: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                stream_failed: AtomicBool::new(false),
                configured: Mutex::new(None),
                producer: Mutex::new(None),
                cancel_waiters: Mutex::new(Vec::new()),
                stream: Mutex::new(None),
                converter: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn sink(&self) -> AudioSinkBox {
        AudioSink_TO::from_value(
            HostAudioSink {
                state: Arc::clone(&self.state),
            },
            TD_Opaque,
        )
    }

    /// Stop the device before releasing the family session. The cancellation
    /// signal disconnects blocked writes; no fixed tick or callback is needed.
    pub(crate) fn close(&self) -> Result<(), String> {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.closed.store(true, Ordering::Release);
        wake_cancel_waiters(&self.state)?;
        self.state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())?
            .take();
        self.state
            .stream
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_LOCK".to_owned())?
            .take();
        if self.state.stream_failed.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED".into());
        }
        Ok(())
    }

    pub(crate) fn check_health(&self) -> Result<(), String> {
        if self.state.stream_failed.load(Ordering::Acquire) {
            Err("ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED".into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.state
            .configured
            .lock()
            .expect("audio state lock")
            .is_some()
    }
}

impl Drop for HostAudioExecutor {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl HostAudioSink {
    fn configure_inner(&self, source: PcmFormatSpec) -> Result<(), String> {
        source.validate().map_err(|error| error.to_string())?;
        if self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
        {
            return Err("ASTRA_EMU_AUDIO_CLOSED".into());
        }
        if self
            .state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK".to_owned())?
            .is_some()
        {
            return Err("ASTRA_EMU_AUDIO_ALREADY_CONFIGURED".into());
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE".to_owned())?;
        // The device's default stream is authoritative. Family PCM is
        // converted to this rate/channel layout before entering the bounded
        // queue, and the callback only performs sample type conversion.
        let supported = device
            .default_output_config()
            .map_err(|_| "ASTRA_EMU_AUDIO_DEFAULT_FORMAT".to_owned())?;
        let output = OutputFormat {
            source,
            device_rate: supported.sample_rate(),
            device_channels: supported.channels(),
            device_sample_format: supported.sample_format(),
        };
        if !matches!(
            output.device_sample_format,
            cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
        ) {
            return Err("ASTRA_EMU_AUDIO_DEVICE_SAMPLE_FORMAT".into());
        }
        let converter = AudioConverter::new(output)?;
        let stream_config: cpal::StreamConfig = supported.clone().into();
        let (producer, receiver) = bounded(AUDIO_QUEUE_CHUNKS);
        let mut consumer = DeviceConsumer {
            receiver,
            blocks: VecDeque::with_capacity(AUDIO_QUEUE_CHUNKS),
            current: None,
            cursor: 0,
        };
        let callback_state = Arc::clone(&self.state);
        let on_error = move |_error: cpal::StreamError| {
            callback_state.stream_failed.store(true, Ordering::Release);
            callback_state.cancelled.store(true, Ordering::Release);
            wake_cancel_waiters_best_effort(&callback_state);
        };
        let stream = match output.device_sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |samples: &mut [f32], _| consumer.fill_f32(samples),
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |samples: &mut [i16], _| consumer.fill_i16(samples),
                on_error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |samples: &mut [u16], _| consumer.fill_u16(samples),
                on_error,
                None,
            ),
            _ => unreachable!("sample format was checked above"),
        }
        .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_CREATE".to_owned())?;
        stream
            .play()
            .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_START".to_owned())?;

        let mut configured = self
            .state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK".to_owned())?;
        if configured.is_some() {
            return Err("ASTRA_EMU_AUDIO_ALREADY_CONFIGURED".into());
        }
        *configured = Some(output);
        drop(configured);
        *self
            .state
            .converter
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONVERTER_LOCK".to_owned())? = Some(converter);
        *self
            .state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())? = Some(producer);
        *self
            .state
            .stream
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_LOCK".to_owned())? = Some(stream);
        tracing::info!(
            event = "astra.emu.audio.configured",
            source_rate = source.sample_rate,
            source_channels = source.channels,
            device_rate = output.device_rate,
            device_channels = output.device_channels,
        );
        Ok(())
    }

    fn write_inner(&self, chunk: PcmChunk) -> Result<AudioWriteStatus, String> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Cancelled);
        }
        if self.state.closed.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Closed);
        }
        let output = (*self
            .state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK".to_owned())?)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?;
        chunk
            .validate(output.source)
            .map_err(|error| error.to_string())?;
        let samples = self
            .state
            .converter
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONVERTER_LOCK".to_owned())?
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?
            .push(pcm_chunk_samples(chunk))?;
        let producer = self
            .state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?;
        // `select!` waits on either queue capacity or cancellation. It never
        // polls the queue with a timer, so Family workers stop promptly when
        // the Manager closes the session.
        let Some((cancel_tx, cancel_rx)) = register_cancel_waiter(&self.state)? else {
            return Ok(AudioWriteStatus::Cancelled);
        };
        let result = select! {
            send(producer, samples) -> result => result.map(|_| AudioWriteStatus::Accepted).map_err(|_| "ASTRA_EMU_AUDIO_QUEUE_CLOSED".into()),
            recv(cancel_rx) -> _ => Ok(AudioWriteStatus::Cancelled),
        };
        unregister_cancel_waiter(&self.state, &cancel_tx)?;
        result
    }
}

impl AudioSink for HostAudioSink {
    fn configure(&self, format: PcmFormatSpec) -> astra_emu_family_api::FfiFamilyResult<()> {
        self.configure_inner(format)
            .map_or_else(
                |error| Err(audio_error("ASTRA_EMU_AUDIO_CONFIG", error)),
                |_| Ok(()),
            )
            .into()
    }

    fn write(&self, chunk: PcmChunk) -> astra_emu_family_api::FfiFamilyResult<AudioWriteStatus> {
        self.write_inner(chunk)
            .map_or_else(|error| Err(audio_error("ASTRA_EMU_AUDIO_WRITE", error)), Ok)
            .into()
    }

    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
    }

    fn cancel(&self) -> astra_emu_family_api::FfiFamilyResult<()> {
        self.state.cancelled.store(true, Ordering::Release);
        wake_cancel_waiters(&self.state)
            .map_or_else(
                |error| Err(audio_error("ASTRA_EMU_AUDIO_CANCEL", error)),
                |_| Ok(()),
            )
            .into()
    }
}
fn audio_error(code: &str, message: String) -> astra_emu_family_api::FamilyError {
    astra_emu_family_api::FamilyError::new(code, message)
}

fn register_cancel_waiter(state: &AudioState) -> Result<Option<CancelWaiter>, String> {
    let (sender, receiver) = bounded(1);
    let mut waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    if state.cancelled.load(Ordering::Acquire) || state.closed.load(Ordering::Acquire) {
        return Ok(None);
    }
    waiters.push(sender.clone());
    Ok(Some((sender, receiver)))
}

fn unregister_cancel_waiter(state: &AudioState, sender: &Sender<()>) -> Result<(), String> {
    let mut waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    waiters.retain(|waiter| !waiter.same_channel(sender));
    Ok(())
}

fn wake_cancel_waiters(state: &AudioState) -> Result<(), String> {
    let waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    for waiter in waiters.iter() {
        let _ = waiter.try_send(());
    }
    Ok(())
}

fn wake_cancel_waiters_best_effort(state: &AudioState) {
    if let Err(error) = wake_cancel_waiters(state) {
        tracing::error!(
            event = "astra.emu.audio.cancel_waiters_failed",
            diagnostic_code = "ASTRA_EMU_AUDIO_CANCEL_LOCK",
            error_kind = %error,
        );
    }
}

impl DeviceConsumer {
    fn refill(&mut self) {
        while self.blocks.len() < AUDIO_QUEUE_CHUNKS {
            match self.receiver.try_recv() {
                Ok(block) => {
                    debug_assert_eq!(self.blocks.capacity(), AUDIO_QUEUE_CHUNKS);
                    self.blocks.push_back(block);
                }
                Err(_) => break,
            }
        }
        if self.current.is_none() {
            self.current = self.blocks.pop_front();
            self.cursor = 0;
        }
    }

    fn next(&mut self) -> f32 {
        loop {
            let Some(current) = self.current.as_ref() else {
                return 0.0;
            };
            if self.cursor < current.len() {
                let value = current[self.cursor];
                self.cursor += 1;
                return value;
            }
            self.current = self.blocks.pop_front();
            self.cursor = 0;
            if self.current.is_none() {
                return 0.0;
            }
        }
    }

    fn fill_f32(&mut self, output: &mut [f32]) {
        self.refill();
        for sample in output {
            *sample = self.next();
        }
    }

    fn fill_i16(&mut self, output: &mut [i16]) {
        self.refill();
        for sample in output {
            *sample = (self.next().clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        }
    }

    fn fill_u16(&mut self, output: &mut [u16]) {
        self.refill();
        for sample in output {
            *sample = ((self.next().clamp(-1.0, 1.0) * 0.5 + 0.5) * 65_535.0).round() as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi_stable::std_types::RVec;

    #[test]
    fn conversion_maps_mono_and_preserves_i16_extremes() {
        let values = convert_chunk(
            PcmChunk::I16(RVec::from(vec![i16::MIN, i16::MAX])),
            OutputFormat {
                source: PcmFormatSpec {
                    sample_rate: 48_000,
                    channels: 1,
                    format: PcmFormat::I16,
                },
                device_rate: 48_000,
                device_channels: 2,
                device_sample_format: cpal::SampleFormat::F32,
            },
        )
        .unwrap();
        assert_eq!(values.len(), 4);
        assert_eq!(values[0], values[1]);
        assert!(values[0] <= -0.99);
        assert_eq!(values[2], values[3]);
        assert_eq!(values[2], 1.0);
    }

    #[test]
    fn conversion_resamples_with_bounded_output() {
        let values = convert_samples(vec![0.0; 480], 48_000, 1, 24_000, 1).unwrap();
        assert!(!values.is_empty());
        assert!(values.len() <= MAX_AUDIO_OUTPUT_SAMPLES);
    }

    #[test]
    fn persistent_resampler_is_invariant_to_irregular_input_chunks() {
        let output = OutputFormat {
            source: PcmFormatSpec {
                sample_rate: 48_000,
                channels: 1,
                format: PcmFormat::F32,
            },
            device_rate: 24_000,
            device_channels: 1,
            device_sample_format: cpal::SampleFormat::F32,
        };
        let input = (0..10_000)
            .map(|index| ((index as f32) * 0.013).sin())
            .collect::<Vec<_>>();
        let mut whole = AudioConverter::new(output).unwrap();
        let expected = whole.push(input.clone()).unwrap();
        let mut irregular = AudioConverter::new(output).unwrap();
        let mut actual = Vec::new();
        for chunk in input.chunks(137) {
            actual.extend(irregular.push(chunk.to_vec()).unwrap());
        }
        assert_eq!(actual.len(), expected.len());
        for (left, right) in actual.iter().zip(expected.iter()) {
            assert!((left - right).abs() < 1.0e-6);
        }
    }

    #[test]
    fn persistent_converter_maps_stereo_to_mono() {
        let output = OutputFormat {
            source: PcmFormatSpec {
                sample_rate: 48_000,
                channels: 2,
                format: PcmFormat::F32,
            },
            device_rate: 48_000,
            device_channels: 1,
            device_sample_format: cpal::SampleFormat::F32,
        };
        let mut converter = AudioConverter::new(output).unwrap();
        assert_eq!(
            converter.push(vec![1.0, -1.0, 0.4, 0.2]).unwrap(),
            vec![0.0, 0.3]
        );
    }

    #[test]
    fn cancellation_wakes_all_registered_writers() {
        let state = AudioState {
            cancelled: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            stream_failed: AtomicBool::new(false),
            configured: Mutex::new(None),
            producer: Mutex::new(None),
            cancel_waiters: Mutex::new(Vec::new()),
            stream: Mutex::new(None),
            converter: Mutex::new(None),
        };
        let (first_tx, first) = register_cancel_waiter(&state).unwrap().unwrap();
        let (second_tx, second) = register_cancel_waiter(&state).unwrap().unwrap();
        state.cancelled.store(true, Ordering::Release);
        wake_cancel_waiters(&state).unwrap();
        assert!(first.recv().is_ok());
        assert!(second.recv().is_ok());
        drop((first_tx, second_tx));
    }

    #[test]
    fn stream_error_is_reported_by_health_check() {
        let executor = HostAudioExecutor::new();
        executor.state.stream_failed.store(true, Ordering::Release);
        assert_eq!(
            executor.check_health().unwrap_err(),
            "ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED"
        );
    }

    #[test]
    fn close_cancels_unconfigured_sink_without_device_access() {
        let executor = HostAudioExecutor::new();
        assert!(!executor.is_configured());
        executor.close().unwrap();
        assert!(executor.sink().is_cancelled());
    }
}
