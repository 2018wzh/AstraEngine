#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl PlayerDecodedAudio {
    pub fn parse(
        format: &str,
        bytes: &[u8],
        max_samples: usize,
    ) -> Result<Self, PlayerAudioContractError> {
        let mut parts = format.split(':');
        let codec = parts.next().unwrap_or_default();
        let sample_rate = parse_number::<u32>(parts.next(), "ASTRA_PLAYER_AUDIO_SAMPLE_RATE")?;
        let channels = parse_number::<u16>(parts.next(), "ASTRA_PLAYER_AUDIO_CHANNELS")?;
        if parts.next().is_some() {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_FORMAT_UNSUPPORTED",
                "decoded audio format has unexpected fields",
            ));
        }
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
        let samples = match codec {
            "pcm_s16le" => decode_s16le(bytes, max_samples)?,
            _ => {
                return Err(PlayerAudioContractError::new(
                    "ASTRA_PLAYER_AUDIO_FORMAT_UNSUPPORTED",
                    format!("decoded audio format {codec} is unsupported"),
                ));
            }
        };
        if samples.len() % usize::from(channels) != 0 {
            return Err(PlayerAudioContractError::new(
                "ASTRA_PLAYER_AUDIO_FRAME_ALIGNMENT",
                "decoded audio samples do not contain complete frames",
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
}

fn decode_s16le(bytes: &[u8], max_samples: usize) -> Result<Vec<f32>, PlayerAudioContractError> {
    if bytes.len() % 2 != 0 {
        return Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_PCM_TRUNCATED",
            "decoded PCM payload ends inside a sample",
        ));
    }
    let sample_count = bytes.len() / 2;
    if sample_count > max_samples {
        return Err(PlayerAudioContractError::new(
            "ASTRA_PLAYER_AUDIO_SAMPLE_BUDGET",
            "decoded audio exceeds the configured sample budget",
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|sample| {
            let value = i16::from_le_bytes([sample[0], sample[1]]);
            if value < 0 {
                f32::from(value) / 32768.0
            } else {
                f32::from(value) / 32767.0
            }
        })
        .collect())
}

fn parse_number<T: std::str::FromStr>(
    value: Option<&str>,
    code: &'static str,
) -> Result<T, PlayerAudioContractError> {
    value.and_then(|value| value.parse().ok()).ok_or_else(|| {
        PlayerAudioContractError::new(code, "decoded audio format contains an invalid number")
    })
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
