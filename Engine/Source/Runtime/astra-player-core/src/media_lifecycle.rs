use astra_core::Hash256;

use crate::{
    PlayerHostCommandBatch, PlayerHostCommandExecutor, PlayerHostCommandResult,
    PlayerHostCommandSink, PlayerHostResourceId,
};

pub struct PlayerDecodeLifecyclePlan {
    pub session: PlayerHostResourceId,
    pub open: PlayerHostCommandBatch,
    pub decode: PlayerHostCommandBatch,
    pub close: PlayerHostCommandBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDecodedBuffer {
    pub format: String,
    pub hash: String,
    pub bytes: Vec<u8>,
}

pub struct PlayerAudioLifecyclePlan {
    pub output: PlayerHostResourceId,
    pub expected_sample_count: u64,
    pub open: PlayerHostCommandBatch,
    pub submits: Vec<PlayerHostCommandBatch>,
    pub drain: PlayerHostCommandBatch,
    pub close: PlayerHostCommandBatch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAudioPlaybackEvidence {
    pub output: PlayerHostResourceId,
    pub sample_count: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

impl<S> PlayerHostCommandExecutor<S>
where
    S: PlayerHostCommandSink,
    S::Error: std::fmt::Display,
{
    pub async fn execute_decode_open(
        &mut self,
        session: PlayerHostResourceId,
        open: PlayerHostCommandBatch,
    ) -> Result<(), PlayerMediaLifecycleError> {
        let results = self.execute_batch(open).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_OPEN", error.to_string())
        })?;
        if !matches!(
            results.as_slice(),
            [PlayerHostCommandResult::DecodeOpened { session: opened }] if *opened == session
        ) {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_OPEN_RESULT",
                "decode open returned an invalid logical resource",
            ));
        }
        Ok(())
    }

    pub async fn execute_decode_submit(
        &mut self,
        session: PlayerHostResourceId,
        decode: PlayerHostCommandBatch,
    ) -> Result<PlayerDecodedBuffer, PlayerMediaLifecycleError> {
        let results = self.execute_batch(decode).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_SUBMIT", error.to_string())
        })?;
        let [PlayerHostCommandResult::Decoded {
            session: decoded_session,
            format,
            hash,
            bytes,
        }] = results.as_slice()
        else {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_RESULT",
                "decode submit returned an invalid result",
            ));
        };
        if *decoded_session != session {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_RESULT",
                "decode submit returned the wrong logical resource",
            ));
        }
        if Hash256::from_sha256(bytes).to_string() != *hash {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_HASH",
                "decoded buffer hash does not match its bytes",
            ));
        }
        Ok(PlayerDecodedBuffer {
            format: format.clone(),
            hash: hash.clone(),
            bytes: bytes.clone(),
        })
    }

    pub async fn execute_decode_close(
        &mut self,
        session: PlayerHostResourceId,
        close: PlayerHostCommandBatch,
    ) -> Result<(), PlayerMediaLifecycleError> {
        let results = self.execute_batch(close).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_CLOSE", error.to_string())
        })?;
        if !matches!(
            results.as_slice(),
            [PlayerHostCommandResult::DecodeClosed { session: closed }] if *closed == session
        ) {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_CLOSE_RESULT",
                "decode close returned an invalid logical resource",
            ));
        }
        Ok(())
    }

    pub async fn execute_decode_lifecycle(
        &mut self,
        plan: PlayerDecodeLifecyclePlan,
    ) -> Result<PlayerDecodedBuffer, PlayerMediaLifecycleError> {
        if let Err(error) = self.execute_decode_open(plan.session, plan.open).await {
            return Err(self.cleanup_decode(plan.close, plan.session, error).await);
        }
        let decoded = match self.execute_decode_submit(plan.session, plan.decode).await {
            Ok(decoded) => decoded,
            Err(error) => {
                return Err(self.cleanup_decode(plan.close, plan.session, error).await);
            }
        };
        self.execute_decode_close(plan.session, plan.close).await?;
        Ok(decoded)
    }

    async fn cleanup_decode(
        &mut self,
        close: PlayerHostCommandBatch,
        expected_session: PlayerHostResourceId,
        mut error: PlayerMediaLifecycleError,
    ) -> PlayerMediaLifecycleError {
        match self.execute_batch(close).await {
            Ok(results) if matches!(results.as_slice(), [PlayerHostCommandResult::DecodeClosed { session }] if *session == expected_session) =>
                {}
            Ok(_) => {
                error.cleanup_error =
                    Some("decode close returned an invalid logical resource".into())
            }
            Err(cleanup) => error.cleanup_error = Some(cleanup.to_string()),
        }
        error
    }

    pub async fn execute_audio_lifecycle(
        &mut self,
        plan: PlayerAudioLifecyclePlan,
    ) -> Result<PlayerAudioPlaybackEvidence, PlayerMediaLifecycleError> {
        let open = self.execute_batch(plan.open).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_AUDIO_OPEN", error.to_string())
        })?;
        if !matches!(
            open.as_slice(),
            [PlayerHostCommandResult::AudioOpened { output }] if *output == plan.output
        ) {
            return Err(self
                .cleanup_audio(
                    plan.close,
                    plan.output,
                    PlayerMediaLifecycleError::new(
                        "ASTRA_PLAYER_AUDIO_OPEN_RESULT",
                        "audio open returned an invalid logical resource",
                    ),
                )
                .await);
        }
        for submit in plan.submits {
            match self.execute_batch(submit).await {
                Ok(results) if matches!(results.as_slice(), [PlayerHostCommandResult::Unit]) => {}
                Ok(_) => {
                    return Err(self
                        .cleanup_audio(
                            plan.close,
                            plan.output,
                            PlayerMediaLifecycleError::new(
                                "ASTRA_PLAYER_AUDIO_SUBMIT_RESULT",
                                "audio submit returned an invalid result",
                            ),
                        )
                        .await);
                }
                Err(error) => {
                    return Err(self
                        .cleanup_audio(
                            plan.close,
                            plan.output,
                            PlayerMediaLifecycleError::new(
                                "ASTRA_PLAYER_AUDIO_SUBMIT",
                                error.to_string(),
                            ),
                        )
                        .await);
                }
            }
        }
        let drained = match self.execute_batch(plan.drain).await {
            Ok(results) => match results.as_slice() {
                [PlayerHostCommandResult::AudioDrained {
                    output,
                    sample_count,
                    peak_dbfs_bits,
                    rms_dbfs_bits,
                }] if *output == plan.output => {
                    let peak_dbfs = f32::from_bits(*peak_dbfs_bits);
                    let rms_dbfs = f32::from_bits(*rms_dbfs_bits);
                    if *sample_count != plan.expected_sample_count
                        || !peak_dbfs.is_finite()
                        || !rms_dbfs.is_finite()
                        || peak_dbfs > 0.0
                        || rms_dbfs > 0.0
                    {
                        return Err(self
                            .cleanup_audio(
                                plan.close,
                                plan.output,
                                PlayerMediaLifecycleError::new(
                                    "ASTRA_PLAYER_AUDIO_METER_INVALID",
                                    "audio drain meter does not match submitted samples",
                                ),
                            )
                            .await);
                    }
                    PlayerAudioPlaybackEvidence {
                        output: *output,
                        sample_count: *sample_count,
                        peak_dbfs,
                        rms_dbfs,
                    }
                }
                _ => {
                    return Err(self
                        .cleanup_audio(
                            plan.close,
                            plan.output,
                            PlayerMediaLifecycleError::new(
                                "ASTRA_PLAYER_AUDIO_DRAIN_RESULT",
                                "audio drain returned an invalid result",
                            ),
                        )
                        .await);
                }
            },
            Err(error) => {
                return Err(self
                    .cleanup_audio(
                        plan.close,
                        plan.output,
                        PlayerMediaLifecycleError::new(
                            "ASTRA_PLAYER_AUDIO_DRAIN",
                            error.to_string(),
                        ),
                    )
                    .await);
            }
        };
        let close = self.execute_batch(plan.close).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_AUDIO_CLOSE", error.to_string())
        })?;
        if !matches!(
            close.as_slice(),
            [PlayerHostCommandResult::AudioClosed { output }] if *output == plan.output
        ) {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_AUDIO_CLOSE_RESULT",
                "audio close returned an invalid logical resource",
            ));
        }
        Ok(drained)
    }

    async fn cleanup_audio(
        &mut self,
        close: PlayerHostCommandBatch,
        expected_output: PlayerHostResourceId,
        mut error: PlayerMediaLifecycleError,
    ) -> PlayerMediaLifecycleError {
        match self.execute_batch(close).await {
            Ok(results) if matches!(results.as_slice(), [PlayerHostCommandResult::AudioClosed { output }] if *output == expected_output) =>
                {}
            Ok(_) => {
                error.cleanup_error =
                    Some("audio close returned an invalid logical resource".into())
            }
            Err(cleanup) => error.cleanup_error = Some(cleanup.to_string()),
        }
        error
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerMediaLifecycleError {
    code: &'static str,
    message: String,
    cleanup_error: Option<String>,
}

impl PlayerMediaLifecycleError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cleanup_error: None,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for PlayerMediaLifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)?;
        if let Some(cleanup) = &self.cleanup_error {
            write!(formatter, "; cleanup failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlayerMediaLifecycleError {}
