/// Canonical decoded PCM owned by the Media layer and reused by Player adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl PlayerDecodedAudio {
    pub fn from_i16(
        sample_rate: u32,
        channels: u16,
        samples: Vec<i16>,
        max_samples: usize,
    ) -> Result<Self, PlayerAudioContractError> {
        validate_format(sample_rate, channels, samples.len(), max_samples)?;
        Ok(Self {
            sample_rate,
            channels,
            samples: samples
                .into_iter()
                .map(|sample| {
                    if sample < 0 {
                        f32::from(sample) / 32768.0
                    } else {
                        f32::from(sample) / 32767.0
                    }
                })
                .collect(),
        })
    }

    pub fn from_f32(
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
        max_samples: usize,
    ) -> Result<Self, PlayerAudioContractError> {
        validate_format(sample_rate, channels, samples.len(), max_samples)?;
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_NON_FINITE",
                "decoded audio contains a non-finite sample",
            ));
        }
        Ok(Self {
            sample_rate,
            channels,
            samples,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    pub fn convert_to(
        &self,
        sample_rate: u32,
        channels: u16,
        max_output_samples: usize,
    ) -> Result<Self, PlayerAudioContractError> {
        use rubato::{
            audioadapter::Adapter, audioadapter_buffers::direct::SequentialSliceOfVecs, Async,
            FixedAsync, PolynomialDegree, Resampler, SincInterpolationParameters,
            SincInterpolationType, WindowFunction,
        };

        if !(8_000..=384_000).contains(&sample_rate) || !(1..=8).contains(&channels) {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_OUTPUT_FORMAT",
                "target audio format is outside the supported range",
            ));
        }
        if self.frame_count() == 0 || max_output_samples == 0 {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_CONVERSION_EMPTY",
                "audio conversion input or output budget is empty",
            ));
        }
        let planar = map_channels(self, channels)?;
        let output_planar = if self.sample_rate == sample_rate {
            planar
        } else {
            let ratio = f64::from(sample_rate) / f64::from(self.sample_rate);
            let mut resampler: Box<dyn Resampler<f32>> = if sample_rate > self.sample_rate {
                Box::new(
                    Async::<f32>::new_poly(
                        ratio,
                        1.0,
                        PolynomialDegree::Septic,
                        1_024,
                        usize::from(channels),
                        FixedAsync::Input,
                    )
                    .map_err(|error| {
                        PlayerAudioContractError::new(
                            "ASTRA_PLAYER_AUDIO_RESAMPLER_CREATE",
                            error.to_string(),
                        )
                    })?,
                )
            } else {
                Box::new(
                    Async::<f32>::new_sinc(
                        ratio,
                        1.0,
                        &SincInterpolationParameters {
                            sinc_len: 128,
                            f_cutoff: Some(0.95),
                            interpolation: SincInterpolationType::Cubic,
                            oversampling_factor: 256,
                            window: WindowFunction::BlackmanHarris2,
                        },
                        1_024,
                        usize::from(channels),
                        FixedAsync::Input,
                    )
                    .map_err(|error| {
                        PlayerAudioContractError::new(
                            "ASTRA_PLAYER_AUDIO_RESAMPLER_CREATE",
                            error.to_string(),
                        )
                    })?,
                )
            };
            let input =
                SequentialSliceOfVecs::new(&planar, usize::from(channels), self.frame_count())
                    .map_err(|error| {
                        PlayerAudioContractError::new(
                            "ASTRA_PLAYER_AUDIO_RESAMPLER_INPUT",
                            error.to_string(),
                        )
                    })?;
            let output = resampler
                .process_all(&input, self.frame_count(), None)
                .map_err(|error| {
                    PlayerAudioContractError::new("ASTRA_PLAYER_AUDIO_RESAMPLE", error.to_string())
                })?;
            let output_samples =
                output
                    .frames()
                    .checked_mul(output.channels())
                    .ok_or_else(|| {
                        PlayerAudioContractError::new(
                            "ASTRA_PLAYER_AUDIO_CONVERSION_OVERFLOW",
                            "resampled output size overflowed",
                        )
                    })?;
            if output_samples > max_output_samples {
                return Err(PlayerAudioContractError::new(
                    "ASTRA_PLAYER_AUDIO_CONVERSION_BUDGET",
                    "resampled audio exceeds the configured sample budget",
                ));
            }
            (0..output.channels())
                .map(|channel| {
                    (0..output.frames())
                        .map(|frame| output.read_sample(channel, frame).unwrap_or_default())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let frame_count = output_planar.first().map_or(0, Vec::len);
        let output_samples = frame_count
            .checked_mul(usize::from(channels))
            .ok_or_else(|| {
                PlayerAudioContractError::new(
                    "ASTRA_PLAYER_AUDIO_CONVERSION_OVERFLOW",
                    "converted output size overflowed",
                )
            })?;
        if output_samples == 0 || output_samples > max_output_samples {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_CONVERSION_BUDGET",
                "converted audio is empty or exceeds the configured sample budget",
            ));
        }
        let mut samples = Vec::with_capacity(output_samples);
        for frame in 0..frame_count {
            for channel in &output_planar {
                samples.push(channel[frame]);
            }
        }
        Ok(Self {
            sample_rate,
            channels,
            samples,
        })
    }

    pub fn into_converted(
        self,
        sample_rate: u32,
        channels: u16,
        max_output_samples: usize,
    ) -> Result<Self, PlayerAudioContractError> {
        if self.sample_rate == sample_rate && self.channels == channels {
            if self.samples.is_empty()
                || self.samples.len() > max_output_samples
                || !self.samples.len().is_multiple_of(usize::from(channels))
                || self.samples.iter().any(|sample| !sample.is_finite())
            {
                return Err(PlayerAudioContractError::new(
                    "ASTRA_PLAYER_AUDIO_CONVERSION_BUDGET",
                    "canonical audio is invalid or exceeds the configured sample budget",
                ));
            }
            return Ok(self);
        }
        self.convert_to(sample_rate, channels, max_output_samples)
    }
}

fn validate_format(
    sample_rate: u32,
    channels: u16,
    sample_count: usize,
    max_samples: usize,
) -> Result<(), PlayerAudioContractError> {
    if !(8_000..=384_000).contains(&sample_rate) {
        return Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_SAMPLE_RATE",
            "decoded audio sample rate is outside the supported range",
        ));
    }
    if !(1..=8).contains(&channels) {
        return Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_CHANNELS",
            "decoded audio channel count is outside the supported range",
        ));
    }
    if sample_count == 0
        || sample_count > max_samples
        || !sample_count.is_multiple_of(usize::from(channels))
    {
        return Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_SAMPLE_BUDGET",
            "decoded audio is empty, misaligned, or exceeds the configured sample budget",
        ));
    }
    Ok(())
}

fn map_channels(
    audio: &PlayerDecodedAudio,
    target_channels: u16,
) -> Result<Vec<Vec<f32>>, PlayerAudioContractError> {
    let source_channels = usize::from(audio.channels);
    let target_channels = usize::from(target_channels);
    let frames = audio.frame_count();
    match (source_channels, target_channels) {
        (source, target) if source == target => Ok((0..source)
            .map(|channel| {
                (0..frames)
                    .map(|frame| audio.samples[frame * source + channel])
                    .collect()
            })
            .collect()),
        (1, 2) => {
            let mono = audio.samples.clone();
            Ok(vec![mono.clone(), mono])
        }
        (2, 1) => Ok(vec![(0..frames)
            .map(|frame| (audio.samples[frame * 2] + audio.samples[frame * 2 + 1]) * 0.5)
            .collect()]),
        _ => Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_CHANNEL_LAYOUT_UNSUPPORTED",
            format!(
                "cannot convert {} channels to {target_channels} without channel layout metadata",
                audio.channels
            ),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerAudioContractError {
    pub code: &'static str,
    pub message: String,
}

impl PlayerAudioContractError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PlayerAudioContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PlayerAudioContractError {}
