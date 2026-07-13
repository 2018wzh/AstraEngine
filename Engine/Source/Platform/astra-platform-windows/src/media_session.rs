use astra_media::{
    DecodedMediaPacket, FfmpegAudioOutputFormat, FfmpegPlaybackDecoder, FfmpegStreamLimits,
    MediaPipelineLimits, MediaPipelineTickOutput, MediaPlaybackPipeline, MediaPlaybackState,
    MediaTrackKind, PlaybackTickRequest, QueuedMediaOutput,
};
mod support;

use support::*;

use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioOutputStatus, AudioPacket, PlatformError,
    PlatformErrorCode, PlatformHostClient, RgbaFrame, SurfaceHandle,
};

pub struct WindowsNativeMediaSession {
    client: PlatformHostClient,
    surface: SurfaceHandle,
    decoder: FfmpegPlaybackDecoder,
    pipeline: MediaPlaybackPipeline,
    audio: Option<AudioState>,
    pending_decoded: Option<DecodedMediaPacket>,
    eof_marked: bool,
    next_surface_sequence: u64,
    failed: bool,
}

struct AudioState {
    handle: AudioOutputHandle,
    sample_rate: u32,
    channels: u16,
    origin_pts_us: u64,
    next_sequence: u64,
    capacity_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PumpProgress {
    Progressed,
    Backpressured,
    Eos,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowsMediaTickOutput {
    pub playback: MediaPipelineTickOutput,
    pub audio_status: Option<AudioOutputStatus>,
    pub presented_surface_sequence: Option<u64>,
    pub audio_recovered: bool,
}

impl WindowsNativeMediaSession {
    pub async fn open(
        client: PlatformHostClient,
        surface: SurfaceHandle,
        codec: &str,
        bytes: &[u8],
        stream_limits: FfmpegStreamLimits,
        pipeline_limits: MediaPipelineLimits,
    ) -> Result<Self, PlatformError> {
        validate_profile(&client)?;
        let mut decoder = FfmpegPlaybackDecoder::open(codec, bytes, stream_limits)
            .map_err(|error| media_error("media.open", error))?;
        if decoder.playback_config().has_audio {
            let audio_format = client.query_audio_device_format().await?;
            decoder
                .configure_audio_output(FfmpegAudioOutputFormat {
                    sample_rate: audio_format.sample_rate,
                    channels: audio_format.channels,
                })
                .map_err(|error| media_error("media.open", error))?;
        }
        let pipeline = MediaPlaybackPipeline::new(decoder.playback_config(), pipeline_limits)
            .map_err(|error| media_error("media.open", error))?;
        let mut session = Self {
            client,
            surface,
            decoder,
            pipeline,
            audio: None,
            pending_decoded: None,
            eof_marked: false,
            next_surface_sequence: 1,
            failed: false,
        };
        if let Err(error) = session.pump_until_initial_buffer().await {
            return Err(session.fail(error).await);
        }
        if let Err(error) = session.pump_available().await {
            return Err(session.fail(error).await);
        }
        if let Err(error) = session
            .pipeline
            .play()
            .map_err(|error| media_error("media.play", error))
        {
            return Err(session.fail(error).await);
        }
        if let Some(audio) = &session.audio {
            if let Err(error) = session.client.resume_audio(audio.handle).await {
                return Err(session.fail(error).await);
            }
        }
        Ok(session)
    }

    pub fn state(&self) -> MediaPlaybackState {
        self.pipeline.scheduler().state
    }

    pub fn generation(&self) -> u64 {
        self.pipeline.scheduler().generation
    }

    #[cfg(feature = "platform-test-driver")]
    pub fn audio_output_handle_for_test(&self) -> Option<AudioOutputHandle> {
        self.audio.as_ref().map(|audio| audio.handle)
    }

    pub async fn tick(&mut self, delta_us: u64) -> Result<WindowsMediaTickOutput, PlatformError> {
        self.ensure_live("media.tick")?;
        let (audio_status, audio_recovered) = self.query_audio_status().await?;
        let audio_playhead_us = match (&self.audio, &audio_status) {
            (Some(audio), Some(status)) => Some(
                audio
                    .origin_pts_us
                    .checked_add(
                        status
                            .played_frames
                            .checked_mul(1_000_000)
                            .map(|value| value / u64::from(audio.sample_rate))
                            .ok_or_else(|| media_clock_error("audio playhead overflowed"))?,
                    )
                    .ok_or_else(|| media_clock_error("audio origin overflowed"))?,
            ),
            (None, None) => None,
            _ => {
                return Err(PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "media.tick",
                    "native media audio status does not match its output resource",
                ))
            }
        };
        let sequence = self
            .pipeline
            .scheduler()
            .last_tick_sequence
            .checked_add(1)
            .ok_or_else(|| media_clock_error("media tick sequence overflowed"))?;
        let playback = self
            .pipeline
            .tick(PlaybackTickRequest {
                sequence,
                delta_us,
                audio_playhead_us,
            })
            .map_err(|error| media_error("media.tick", error))?;
        let presented_surface_sequence = if let Some(frame) = &playback.presented_video {
            let sequence = self.next_surface_sequence;
            let next = sequence.checked_add(1).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "media.present",
                    "surface frame sequence overflowed",
                )
            })?;
            let rgba8 = bgra_to_rgba(&frame.bgra8)?;
            if let Err(error) = self
                .client
                .present_rgba(
                    self.surface,
                    RgbaFrame {
                        sequence,
                        width: frame.packet.width,
                        height: frame.packet.height,
                        rgba8,
                    },
                )
                .await
            {
                return Err(self.fail(error).await);
            }
            self.next_surface_sequence = next;
            Some(sequence)
        } else {
            None
        };
        if !playback.scheduler.ended {
            if let Err(error) = self.pump_available().await {
                return Err(self.fail(error).await);
            }
        }
        Ok(WindowsMediaTickOutput {
            playback,
            audio_status,
            presented_surface_sequence,
            audio_recovered,
        })
    }

    pub async fn pause(&mut self) -> Result<(), PlatformError> {
        self.ensure_live("media.pause")?;
        if self.pipeline.scheduler().state != MediaPlaybackState::Playing {
            return Err(invalid_state("media.pause", "media session is not playing"));
        }
        if let Some(audio) = &self.audio {
            self.client.pause_audio(audio.handle).await?;
        }
        if let Err(error) = self
            .pipeline
            .pause()
            .map_err(|error| media_error("media.pause", error))
        {
            if let Some(audio) = &self.audio {
                if self.client.resume_audio(audio.handle).await.is_err() {
                    let mut error = error;
                    error
                        .fields
                        .insert("rollback".to_string(), "audio_resume_failed".to_string());
                    self.failed = true;
                    return Err(error);
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub async fn resume(&mut self) -> Result<(), PlatformError> {
        self.ensure_live("media.resume")?;
        if self.pipeline.scheduler().state != MediaPlaybackState::Paused {
            return Err(invalid_state("media.resume", "media session is not paused"));
        }
        if let Some(audio) = &self.audio {
            self.client.resume_audio(audio.handle).await?;
        }
        if let Err(error) = self
            .pipeline
            .play()
            .map_err(|error| media_error("media.resume", error))
        {
            if let Some(audio) = &self.audio {
                if self.client.pause_audio(audio.handle).await.is_err() {
                    let mut error = error;
                    error
                        .fields
                        .insert("rollback".to_string(), "audio_pause_failed".to_string());
                    self.failed = true;
                    return Err(error);
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub async fn seek(&mut self, position_us: u64) -> Result<u64, PlatformError> {
        self.ensure_live("media.seek")?;
        if matches!(
            self.pipeline.scheduler().state,
            MediaPlaybackState::Seeking | MediaPlaybackState::Cancelled
        ) || position_us > self.pipeline.scheduler().config.duration_us
        {
            return Err(invalid_state(
                "media.seek",
                "media seek state or target is invalid",
            ));
        }
        if let Some(audio) = self.audio.take() {
            self.client.abort_audio(audio.handle).await?;
        }
        let generation = self
            .decoder
            .seek(position_us)
            .map_err(|error| media_error("media.seek", error))?;
        let pipeline_generation = self
            .pipeline
            .begin_seek(position_us)
            .map_err(|error| media_error("media.seek", error))?;
        if generation != pipeline_generation {
            return Err(self
                .fail(PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "media.seek",
                    "decoder and scheduler generations diverged",
                ))
                .await);
        }
        self.pending_decoded = None;
        self.eof_marked = false;
        if let Err(error) = self.pump_until_initial_buffer().await {
            return Err(self.fail(error).await);
        }
        if let Err(error) = self.pump_available().await {
            return Err(self.fail(error).await);
        }
        self.pipeline
            .complete_seek(generation)
            .map_err(|error| media_error("media.seek", error))?;
        if self.pipeline.scheduler().state == MediaPlaybackState::Playing {
            if let Some(audio) = &self.audio {
                self.client.resume_audio(audio.handle).await?;
            }
        }
        Ok(generation)
    }

    pub async fn shutdown(mut self) -> Result<(), PlatformError> {
        self.ensure_live("media.shutdown")?;
        let ended = self.pipeline.scheduler().state == MediaPlaybackState::Ended;
        let mut failure = self
            .decoder
            .cancel()
            .err()
            .map(|error| media_error("media.shutdown", error));
        if !ended {
            if let Err(error) = self.pipeline.cancel() {
                append_cleanup_failure(
                    &mut failure,
                    media_error("media.shutdown", error),
                    "pipeline_cancel_failed",
                );
            }
        }
        if let Some(audio) = self.audio.take() {
            let result = if ended {
                self.client.close_audio(audio.handle).await
            } else {
                self.client.abort_audio(audio.handle).await
            };
            if let Err(error) = result {
                append_cleanup_failure(&mut failure, error, "audio_release_failed");
            }
        }
        self.failed = true;
        failure.map_or(Ok(()), Err)
    }

    async fn pump_until_initial_buffer(&mut self) -> Result<(), PlatformError> {
        loop {
            if initial_buffer_ready(&self.pipeline) {
                return Ok(());
            }
            match self.pump_one().await? {
                PumpProgress::Progressed => {}
                PumpProgress::Backpressured => {
                    return Err(PlatformError::new(
                        PlatformErrorCode::QueueOverflow,
                        "media.buffer",
                        "profile media queues cannot hold the decoder pre-roll for every track",
                    ))
                }
                PumpProgress::Eos => {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "media.buffer",
                        "decoder reached EOS before every enabled track produced initial data",
                    ))
                }
            }
        }
    }

    async fn query_audio_status(
        &mut self,
    ) -> Result<(Option<AudioOutputStatus>, bool), PlatformError> {
        let Some(handle) = self.audio.as_ref().map(|audio| audio.handle) else {
            return Ok((None, false));
        };
        match self.client.query_audio_output(handle).await {
            Ok(status) => Ok((Some(status), false)),
            Err(error) if error.code == PlatformErrorCode::DeviceLost => {
                if let Err(recovery_error) = self.recover_audio_device().await {
                    return Err(self.fail(recovery_error).await);
                }
                let recovered = self.audio.as_ref().ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "media.audio_recover",
                        "audio recovery completed without an output resource",
                    )
                })?;
                let status = self.client.query_audio_output(recovered.handle).await?;
                Ok((Some(status), true))
            }
            Err(error) => Err(error),
        }
    }

    async fn recover_audio_device(&mut self) -> Result<(), PlatformError> {
        let old = self.audio.take().ok_or_else(|| {
            invalid_state(
                "media.audio_recover",
                "audio recovery requires a live output resource",
            )
        })?;
        let format = self.client.query_audio_device_format().await?;
        if format.sample_rate != old.sample_rate || format.channels != old.channels {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                "media.audio_recover",
                "replacement audio device format differs from the active decoder output",
            ));
        }
        let position_us = self.pipeline.scheduler().position_us;
        let decoder_generation = self
            .decoder
            .seek(position_us)
            .map_err(|error| media_error("media.audio_recover", error))?;
        let pipeline_generation = self
            .pipeline
            .begin_seek(position_us)
            .map_err(|error| media_error("media.audio_recover", error))?;
        if decoder_generation != pipeline_generation {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "media.audio_recover",
                "audio recovery decoder and scheduler generations diverged",
            ));
        }
        self.pending_decoded = None;
        self.eof_marked = false;
        self.pump_until_initial_buffer().await?;
        self.pump_available().await?;
        self.pipeline
            .complete_seek(decoder_generation)
            .map_err(|error| media_error("media.audio_recover", error))?;
        if self.pipeline.scheduler().state == MediaPlaybackState::Playing {
            let audio = self.audio.as_ref().ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "media.audio_recover",
                    "audio recovery did not recreate its output resource",
                )
            })?;
            self.client.resume_audio(audio.handle).await?;
        }
        Ok(())
    }

    async fn pump_available(&mut self) -> Result<(), PlatformError> {
        let capacity = self
            .pipeline
            .scheduler()
            .config
            .max_audio_packets
            .checked_add(self.pipeline.scheduler().config.max_video_frames)
            .ok_or_else(|| invalid_state("media.buffer", "media queue capacity overflowed"))?;
        for _ in 0..capacity {
            if self.pump_one().await? != PumpProgress::Progressed {
                break;
            }
        }
        Ok(())
    }

    async fn pump_one(&mut self) -> Result<PumpProgress, PlatformError> {
        if self.eof_marked {
            return Ok(PumpProgress::Eos);
        }
        let decoded = match self.pending_decoded.take() {
            Some(decoded) => decoded,
            None => match self
                .decoder
                .read_next()
                .map_err(|error| media_error("media.decode", error))?
            {
                Some(decoded) => decoded,
                None => {
                    let duration = self.pipeline.scheduler().config.duration_us;
                    if self.pipeline.scheduler().config.has_audio {
                        self.pipeline
                            .mark_eos(MediaTrackKind::Audio, duration)
                            .map_err(|error| media_error("media.eos", error))?;
                    }
                    if self.pipeline.scheduler().config.has_video {
                        self.pipeline
                            .mark_eos(MediaTrackKind::Video, duration)
                            .map_err(|error| media_error("media.eos", error))?;
                    }
                    self.eof_marked = true;
                    return Ok(PumpProgress::Eos);
                }
            },
        };
        if !self.can_accept(&decoded).await? {
            self.pending_decoded = Some(decoded);
            return Ok(PumpProgress::Backpressured);
        }
        if let DecodedMediaPacket::Audio { packet, .. } = &decoded {
            self.ensure_audio_output(packet).await?;
        }
        let output = self
            .pipeline
            .queue_decoded(decoded)
            .map_err(|error| media_error("media.buffer", error))?;
        if let QueuedMediaOutput::Audio { packet, pcm_s16le } = output {
            let audio = self.audio.as_mut().ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "media.buffer",
                    "decoded audio has no native output",
                )
            })?;
            let sequence = audio.next_sequence;
            self.client
                .submit_audio(
                    audio.handle,
                    AudioPacket {
                        sequence,
                        channels: packet.channels,
                        samples: pcm_s16_to_f32(&pcm_s16le)?,
                    },
                )
                .await?;
            audio.next_sequence = sequence
                .checked_add(1)
                .ok_or_else(|| invalid_state("media.buffer", "audio sequence overflowed"))?;
        }
        Ok(PumpProgress::Progressed)
    }

    async fn can_accept(&self, decoded: &DecodedMediaPacket) -> Result<bool, PlatformError> {
        match decoded {
            DecodedMediaPacket::Audio { packet, .. } => {
                if self.pipeline.scheduler().audio_queue.len()
                    >= self.pipeline.scheduler().config.max_audio_packets
                {
                    return Ok(false);
                }
                if let Some(audio) = &self.audio {
                    if packet.sample_rate != audio.sample_rate || packet.channels != audio.channels
                    {
                        return Err(PlatformError::new(
                            PlatformErrorCode::IntegrityMismatch,
                            "media.buffer",
                            "decoded audio format changed inside one stream generation",
                        ));
                    }
                    let status = self.client.query_audio_output(audio.handle).await?;
                    return Ok(status
                        .buffered_frames
                        .checked_add(u64::from(packet.frame_count))
                        .is_some_and(|frames| frames <= audio.capacity_frames));
                }
                Ok(true)
            }
            DecodedMediaPacket::Video { .. } => Ok(self.pipeline.scheduler().video_queue.len()
                < self.pipeline.scheduler().config.max_video_frames),
        }
    }

    async fn ensure_audio_output(
        &mut self,
        packet: &astra_media::AudioFramePacket,
    ) -> Result<(), PlatformError> {
        if self.audio.is_some() {
            return Ok(());
        }
        let capacity = self.client.profile().limits.max_audio_frames;
        let handle = self
            .client
            .open_audio_output(AudioOutputRequest {
                sample_rate: packet.sample_rate,
                channels: packet.channels,
                max_buffered_frames: capacity,
            })
            .await?;
        if let Err(error) = self.client.pause_audio(handle).await {
            let mut error = error;
            if self.client.abort_audio(handle).await.is_err() {
                error
                    .fields
                    .insert("cleanup".to_string(), "audio_abort_failed".to_string());
            }
            return Err(error);
        }
        self.audio = Some(AudioState {
            handle,
            sample_rate: packet.sample_rate,
            channels: packet.channels,
            origin_pts_us: packet.pts_us,
            next_sequence: 1,
            capacity_frames: capacity as u64,
        });
        Ok(())
    }

    fn ensure_live(&self, operation: &'static str) -> Result<(), PlatformError> {
        if self.failed {
            return Err(invalid_state(operation, "native media session is closed"));
        }
        Ok(())
    }

    async fn fail(&mut self, mut root: PlatformError) -> PlatformError {
        self.failed = true;
        if self.decoder.cancel().is_err() {
            root.fields
                .insert("decoder_cleanup".to_string(), "cancel_failed".to_string());
        }
        if self.pipeline.cancel().is_err() {
            root.fields
                .insert("pipeline_cleanup".to_string(), "cancel_failed".to_string());
        }
        if let Some(audio) = self.audio.take() {
            if self.client.abort_audio(audio.handle).await.is_err() {
                root.fields
                    .insert("cleanup".to_string(), "audio_abort_failed".to_string());
            }
        }
        root
    }
}
