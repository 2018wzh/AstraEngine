use std::{
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};

use super::{
    wake_cancel_waiters_best_effort, AudioState, DeviceConsumer, Ordering, OutputFormat,
    PcmFormatSpec,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AudioDeviceKind {
    #[default]
    Native,
    Null,
}

impl AudioDeviceKind {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "native" => Ok(Self::Native),
            "null" => Ok(Self::Null),
            _ => Err("ASTRA_EMU_AUDIO_DEVICE_KIND_INVALID".into()),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Null => "null",
        }
    }
}

pub(super) enum AudioOutputDevice {
    Native(cpal::Stream),
    Null(NullAudioDevice),
}

impl AudioOutputDevice {
    pub(super) fn open(
        kind: AudioDeviceKind,
        source: PcmFormatSpec,
        consumer: DeviceConsumer,
        state: Arc<AudioState>,
    ) -> Result<(OutputFormat, Self), String> {
        match kind {
            AudioDeviceKind::Native => open_native(source, consumer, state),
            AudioDeviceKind::Null => {
                let output = OutputFormat {
                    source,
                    device_rate: source.sample_rate,
                    device_channels: source.channels,
                    device_sample_format: cpal::SampleFormat::F32,
                };
                Ok((
                    output,
                    Self::Null(NullAudioDevice::open(output, consumer, state)?),
                ))
            }
        }
    }

    pub(super) fn close(self) -> Result<(), String> {
        match self {
            Self::Native(stream) => {
                drop(stream);
                Ok(())
            }
            Self::Null(mut device) => device.close(),
        }
    }
}

fn open_native(
    source: PcmFormatSpec,
    mut consumer: DeviceConsumer,
    state: Arc<AudioState>,
) -> Result<(OutputFormat, AudioOutputDevice), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE")?;
    let supported = device
        .default_output_config()
        .map_err(|_| "ASTRA_EMU_AUDIO_DEFAULT_FORMAT")?;
    let output = OutputFormat {
        source,
        device_rate: supported.sample_rate(),
        device_channels: supported.channels(),
        device_sample_format: supported.sample_format(),
    };
    let config = supported.into();
    let on_error = move |_error: cpal::StreamError| {
        state.stream_failed.store(true, Ordering::Release);
        state.cancelled.store(true, Ordering::Release);
        wake_cancel_waiters_best_effort(&state);
        if let Some(wake) = &state.wake {
            wake();
        }
    };
    let stream = match output.device_sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |samples: &mut [f32], _| consumer.fill_f32(samples),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |samples: &mut [i16], _| consumer.fill_i16(samples),
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |samples: &mut [u16], _| consumer.fill_u16(samples),
            on_error,
            None,
        ),
        _ => return Err("ASTRA_EMU_AUDIO_DEVICE_SAMPLE_FORMAT".into()),
    }
    .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_CREATE")?;
    stream.play().map_err(|_| "ASTRA_EMU_AUDIO_STREAM_START")?;
    Ok((output, AudioOutputDevice::Native(stream)))
}

/// Explicit test output: drains the same bounded PCM queue on a sample clock.
/// It never opens a native device and never represents audible playback.
pub(super) struct NullAudioDevice {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl NullAudioDevice {
    fn open(
        output: OutputFormat,
        consumer: DeviceConsumer,
        state: Arc<AudioState>,
    ) -> Result<Self, String> {
        let (stop, stopped) = bounded(1);
        let worker = std::thread::Builder::new()
            .name("astra-null-audio".into())
            .spawn(move || consume_null(output, consumer, stopped, state))
            .map_err(|_| "ASTRA_EMU_NULL_AUDIO_THREAD_CREATE")?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    fn close(&mut self) -> Result<(), String> {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "ASTRA_EMU_NULL_AUDIO_THREAD_FAILED")?;
        }
        Ok(())
    }
}

impl Drop for NullAudioDevice {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "astra.emu.null_audio.close_failed", diagnostic_code = %error);
        }
    }
}

fn consume_null(
    output: OutputFormat,
    mut consumer: DeviceConsumer,
    stopped: Receiver<()>,
    state: Arc<AudioState>,
) {
    let frames_per_block = u64::from((output.device_rate / 100).max(1));
    let mut samples = vec![0.0; frames_per_block as usize * usize::from(output.device_channels)];
    let started = Instant::now();
    let mut rendered_frames = 0_u64;
    let mut nonzero_samples = 0_u64;
    loop {
        // Derive the next deadline from elapsed sample frames. A delayed wake
        // skips missed callbacks instead of spinning through a catch-up loop.
        let elapsed_frames =
            started.elapsed().as_nanos() * u128::from(output.device_rate) / 1_000_000_000;
        let next_frame =
            (elapsed_frames / u128::from(frames_per_block) + 1) * u128::from(frames_per_block);
        let deadline_ns = next_frame * 1_000_000_000 / u128::from(output.device_rate);
        let wait_ns = deadline_ns.saturating_sub(started.elapsed().as_nanos());
        if !matches!(
            stopped.recv_timeout(Duration::from_nanos(wait_ns as u64)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        ) || state.cancelled.load(Ordering::Acquire)
        {
            break;
        }
        consumer.fill_f32(&mut samples);
        rendered_frames += frames_per_block;
        nonzero_samples += samples.iter().filter(|sample| **sample != 0.0).count() as u64;
    }
    tracing::info!(
        event = "astra.emu.null_audio.closed",
        rendered_frames,
        nonzero_samples
    );
}
