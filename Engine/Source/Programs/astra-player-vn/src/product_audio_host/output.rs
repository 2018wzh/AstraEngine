use super::*;
use std::{future::Future, pin::Pin};

pub(super) type OpenFuture =
    Pin<Box<dyn Future<Output = Result<astra_platform::OpenedAudioOutput, PlatformError>> + Send>>;
pub(super) type CloseFuture = Pin<Box<dyn Future<Output = Result<(), PlatformError>> + Send>>;

impl NativeVnProductAudioHost {
    pub(crate) async fn set_output_paused(
        &mut self,
        paused: bool,
        executor: &PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if let Some(output) = self.output {
            if paused {
                executor.sink().client().pause_audio(output).await?;
            } else {
                executor.sink().client().resume_audio(output).await?;
            }
        }
        self.output_paused = paused;
        Ok(())
    }

    pub async fn ensure_open(
        &mut self,
        _source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if self.service.is_some() {
            return Ok(());
        }
        if self.output.is_some() {
            return Err(player_platform_error(
                "player.audio.open",
                "ASTRA_PLAYER_AUDIO_OUTPUT_REQUIRES_CLEANUP",
            ));
        }
        let client = executor.sink().client().clone();
        let limits = client.launch_profile().limits();
        let capture_samples = self.retain_evidence_audio
            && matches!(client.launch_profile().kind(), HostKind::Headless);
        if self.pending_open.is_none() {
            let request = AudioOutputRequest {
                sample_rate: CANONICAL_SAMPLE_RATE,
                channels: CANONICAL_CHANNELS,
                chunk_frames: limits.audio_chunk_frames,
                max_buffered_frames: Self::BUFFERED_FRAMES,
                start_paused: self.output_paused,
                capture_samples,
            };
            let owner = client.clone();
            self.pending_open = Some(Box::pin(
                async move { owner.open_audio_output(request).await },
            ));
        }
        let opened = self.finish_open().await?;
        if opened.format.sample_rate != CANONICAL_SAMPLE_RATE
            || opened.format.channels != CANONICAL_CHANNELS
        {
            return Err(player_platform_error(
                "player.audio.open",
                "ASTRA_PLAYER_AUDIO_OUTPUT_FORMAT_DRIFT",
            ));
        }
        let evidence_capture = opened.capture;
        if capture_samples && evidence_capture.is_none() {
            return Err(player_platform_error(
                "player.audio.capture",
                "ASTRA_PLAYER_AUDIO_CAPTURE_MISSING",
            ));
        }
        let output = opened.handle;
        let mut service = AudioServiceSession::new(
            AudioServiceConfig {
                max_voices: Self::MAX_VOICES,
                max_buses: Self::MAX_BUSES,
                max_events: Self::MAX_EVENTS,
                pcm_cache_bytes: limits.audio_pcm_cache_bytes,
            },
            AstraChunkBackendSettings {
                sample_rate: opened.format.sample_rate,
                channels: opened.format.channels,
                chunk_frames: limits.audio_chunk_frames,
                endpoint: opened.lane,
                deterministic_fixed_tick_hz: matches!(
                    client.launch_profile().kind(),
                    HostKind::Headless
                )
                .then_some(60),
            },
        )
        .map_err(|error| player_platform_error("player.audio.kira.create", error))?;
        for (asset, sample_rate, channels, samples) in self.pending_recovery_assets.drain(..) {
            service
                .prepare_pcm_shared(asset, sample_rate, channels, samples)
                .map_err(|error| player_platform_error("player.audio.recover.prepare", error))?;
        }
        if let Some(timeline) = self.pending_restore.take() {
            service
                .restore_timeline(timeline)
                .map_err(|error| player_platform_error("player.audio.restore", error))?;
        }
        self.output = Some(output);
        self.service = Some(service);
        self.evidence_capture = evidence_capture;
        self.previous_telemetry = AudioChunkTelemetry::default();
        Ok(())
    }

    pub(crate) async fn reset_output_after_restore(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if self.service.is_some() {
            self.recover_device_loss(source, executor).await?;
        }
        Ok(())
    }

    /// Recreates the selected output endpoint and Kira manager without copying cached PCM.
    /// Failure is terminal for this audio host; no alternate mixer or output provider is selected.
    pub async fn recover_device_loss(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        let service = self.service.take().ok_or_else(|| {
            player_platform_error(
                "player.audio.recover",
                "ASTRA_PLAYER_AUDIO_RECOVERY_REQUIRES_OPEN_SESSION",
            )
        })?;
        self.pending_restore = Some(service.timeline().clone());
        self.pending_recovery_assets = service.prepared_pcm_assets();
        drop(service);
        self.evidence_capture = None;
        self.previous_telemetry = AudioChunkTelemetry::default();
        if self.output.is_none() {
            return Err(player_platform_error(
                "player.audio.recover",
                "ASTRA_PLAYER_AUDIO_RECOVERY_OUTPUT_MISSING",
            ));
        }
        self.close_output(executor).await?;
        self.ensure_open(source, executor).await
    }

    pub async fn shutdown(
        &mut self,
        _source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if self.pending_open.is_some() {
            drop(self.finish_open().await?);
        }
        if let Some(service) = self.service.as_mut() {
            service
                .poll_backend()
                .map_err(|error| player_platform_error("player.audio.shutdown", error))?;
            self.last_meter = Some(NativeVnAudioMeterSnapshot::from(service.telemetry()));
        }
        drop(self.service.take());
        self.close_output(executor).await?;
        self.prepared_assets.clear();
        self.voice_kinds.clear();
        self.known_bgm_targets.clear();
        self.pending_fade_stops.clear();
        self.pending_restore = None;
        self.pending_recovery_assets.clear();
        Ok(())
    }

    async fn finish_open(&mut self) -> Result<astra_platform::OpenedAudioOutput, PlatformError> {
        let response = self
            .pending_open
            .as_mut()
            .expect("an audio open is pending");
        let result = response.await;
        self.pending_open = None;
        let opened = result?;
        // Retain the handle even if subsequent format or mixer setup fails.
        self.output = Some(opened.handle);
        Ok(opened)
    }

    async fn close_output(
        &mut self,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        let Some(output) = self.output else {
            return Ok(());
        };
        if self.pending_close.is_none() {
            let client = executor.sink().client().clone();
            self.pending_close = Some(Box::pin(async move { client.close_audio(output).await }));
        }
        let result = self
            .pending_close
            .as_mut()
            .expect("an audio close is pending")
            .await;
        self.pending_close = None;
        result?;
        self.output = None;
        Ok(())
    }
}
