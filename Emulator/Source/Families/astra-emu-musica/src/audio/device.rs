use super::*;
use astra_platform::{AudioOutputLane, AudioWakeRegistration};
use astra_platform_common::{NativeAudioConsumer, NativeAudioProducer, NativeAudioQueue};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub(super) struct DeviceOutput {
    producer: NativeAudioProducer,
    failed: Arc<AtomicBool>,
    converter: astra_emu_sdk::NativeAudioConverter,
    partial: Vec<f32>,
    partial_len: usize,
    _stream: cpal::Stream,
}
impl DeviceOutput {
    pub fn open(stop: Arc<AtomicBool>) -> FamilyResult<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_AUDIO_DEVICE",
                    "native output device is unavailable",
                )
            })?;
        // Device negotiation and Rubato conversion retain the old output
        // backend's policy; the VM/Kira/movie source clock remains 48 kHz.
        let supported = device.default_output_config().map_err(|_| {
            error(
                "ASTRA_EMU_AUDIO_DEFAULT_FORMAT",
                "cannot query native device format",
            )
        })?;
        let converter = astra_emu_sdk::NativeAudioConverter::new(
            FORMAT.sample_rate,
            FORMAT.channels,
            supported.sample_rate(),
            supported.channels(),
        )
        .map_err(conversion_error)?;
        let chunk_samples = 512 * usize::from(supported.channels());
        let partial = if converter.needs_conversion() {
            vec![0.0; chunk_samples]
        } else {
            Vec::new()
        };
        let format = supported.sample_format();
        let config = supported.into();
        let wake = AudioWakeRegistration::default();
        let (producer, consumer, _) = NativeAudioQueue::create(4, chunk_samples, wake.clone())
            .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_QUEUE", "cannot create device queue"))?;
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match format {
            cpal::SampleFormat::I8 => {
                build::<i8>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I16 => {
                build::<i16>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I24 => {
                build::<cpal::I24>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I32 => {
                build::<i32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I64 => {
                build::<i64>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U8 => {
                build::<u8>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U16 => {
                build::<u16>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U32 => {
                build::<u32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U64 => {
                build::<u64>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::F32 => {
                build::<f32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::F64 => {
                build::<f64>(&device, config, consumer, wake, failed.clone(), stop)
            }
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_AUDIO_FORMAT",
                    "unsupported native sample format",
                ))
            }
        }
        .map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_AUDIO_STREAM",
                "native stream creation failed",
            )
        })?;
        stream
            .play()
            .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_START", "native stream start failed"))?;
        Ok(Self {
            producer,
            failed,
            converter,
            partial,
            partial_len: 0,
            _stream: stream,
        })
    }
    pub fn check(&self) -> FamilyResult<()> {
        if self.failed.load(Ordering::Acquire) {
            Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_DEVICE",
                "native output device failed",
            ))
        } else {
            Ok(())
        }
    }
    pub fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> FamilyResult<Vec<f32>> {
        // wait_for_capacity observes this independent flag at most every 20 ms.
        // The device error callback also sets it, waking a blocked producer.
        self.check()?;
        if stop.load(Ordering::Acquire) {
            return Ok(samples);
        }
        if self.converter.needs_conversion() {
            let producer = &mut self.producer;
            let partial = &mut self.partial;
            let partial_len = &mut self.partial_len;
            let failed = &self.failed;
            self.converter
                .convert(&samples, |mut converted| {
                    while !converted.is_empty() {
                        if stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let count = (partial.len() - *partial_len).min(converted.len());
                        partial[*partial_len..*partial_len + count]
                            .copy_from_slice(&converted[..count]);
                        *partial_len += count;
                        converted = &converted[count..];
                        if *partial_len == partial.len() {
                            producer
                                .wait_for_capacity(partial.len(), stop)
                                .map_err(|_| "ASTRA_EMU_MUSICA_AUDIO_QUEUE".to_owned())?;
                            if failed.load(Ordering::Acquire) {
                                return Err("ASTRA_EMU_MUSICA_AUDIO_DEVICE".into());
                            }
                            if stop.load(Ordering::Acquire) {
                                return Ok(());
                            }
                            *partial = producer
                                .submit(std::mem::take(partial))
                                .map_err(|_| "ASTRA_EMU_MUSICA_AUDIO_QUEUE".to_owned())?;
                            *partial_len = 0;
                        }
                    }
                    Ok(())
                })
                .map_err(conversion_error)?;
            return Ok(samples);
        }
        self.producer
            .wait_for_capacity(samples.len(), stop)
            .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_QUEUE", "device queue wait failed"))?;
        if self.failed.load(Ordering::Acquire) {
            return Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_DEVICE",
                "native output device failed",
            ));
        }
        if stop.load(Ordering::Acquire) {
            return Ok(samples);
        }
        self.producer.submit(samples).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_AUDIO_QUEUE",
                "device queue submission failed",
            )
        })
    }
}
fn conversion_error(cause: String) -> FamilyError {
    error(
        cause
            .split(':')
            .next()
            .unwrap_or("ASTRA_EMU_MUSICA_AUDIO_CONVERSION"),
        &cause,
    )
}
fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut consumer: NativeAudioConsumer,
    wake: AudioWakeRegistration,
    failed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error> {
    let error_wake = wake.clone();
    device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let mut scratch = [0.0; 1024];
            for chunk in output.chunks_mut(scratch.len()) {
                consumer.fill_output_f32(&mut scratch[..chunk.len()]);
                for (target, source) in chunk.iter_mut().zip(&scratch) {
                    *target = T::from_sample(source.clamp(-1.0, 1.0));
                }
            }
            wake.notify();
        },
        move |_| {
            failed.store(true, Ordering::Release);
            stop.store(true, Ordering::Release);
            error_wake.notify();
        },
        None,
    )
}
