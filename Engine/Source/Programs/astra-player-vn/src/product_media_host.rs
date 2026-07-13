use std::collections::BTreeSet;

use astra_platform::{PlatformError, PlatformErrorCode};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerTimelineCompletion,
    PlayerTimelineScheduler,
};

use crate::{NativeVnAudioOutput, NativeVnHostCommandSource, NativeVnProductAudioHost};

pub struct NativeVnProductMediaHost {
    audio: NativeVnProductAudioHost,
    timeline: PlayerTimelineScheduler,
    completed_signals: BTreeSet<String>,
}

impl Default for NativeVnProductMediaHost {
    fn default() -> Self {
        Self::new(256)
    }
}

impl NativeVnProductMediaHost {
    const MAX_COMPLETION_CHAIN: usize = 1_024;
    const MAX_DECODED_AUDIO_SAMPLES: usize = 10_000_000;

    pub fn new(max_timeline_tasks: usize) -> Self {
        Self {
            audio: NativeVnProductAudioHost::default(),
            timeline: PlayerTimelineScheduler::new(max_timeline_tasks),
            completed_signals: BTreeSet::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.timeline.active_count() > 0 || self.audio.is_active()
    }

    pub async fn poll_and_process(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
    ) -> Result<(), PlatformError> {
        let completed = self
            .timeline
            .poll(now_ms)
            .map_err(|error| media_error("player.timeline.poll", error))?;
        self.process(source, executor, now_ms, completed).await
    }

    pub async fn process(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
        mut completed: Vec<PlayerTimelineCompletion>,
    ) -> Result<(), PlatformError> {
        for _ in 0..Self::MAX_COMPLETION_CHAIN {
            let tasks = source.take_timeline_tasks();
            if !tasks.is_empty() {
                let mut candidate = self.timeline.clone();
                let mut scheduled = Vec::new();
                for task in tasks.iter().cloned() {
                    match candidate.schedule(task, now_ms) {
                        Ok(immediate) => scheduled.extend(immediate),
                        Err(error) => {
                            source.restore_timeline_tasks(tasks);
                            return Err(media_error("player.timeline.schedule", error));
                        }
                    }
                }
                self.timeline = candidate;
                completed.extend(scheduled);
            }

            for completion in std::mem::take(&mut completed) {
                tracing::info!(
                    event = "astra.player.timeline.completed",
                    task_id = %completion.task_id,
                    target = %completion.target,
                    completion = ?completion.kind,
                    completed_at_ms = completion.completed_at_ms,
                    "Player timeline task reached a host completion boundary"
                );
                if let Some(fence) = completion.fence {
                    self.completed_signals.insert(fence);
                }
            }

            for output in source.take_audio_requests() {
                let request = match output {
                    NativeVnAudioOutput::Control(request) => {
                        self.audio.control(&request, &mut self.completed_signals)?;
                        continue;
                    }
                    NativeVnAudioOutput::Start(request) => request,
                };
                let decode = source
                    .prepare_audio_decode(&request)
                    .map_err(|error| media_error("player.audio.decode.prepare", error))?;
                let decoded = executor
                    .execute_decode_lifecycle(decode)
                    .await
                    .map_err(|error| media_error("player.audio.decode", error))?;
                let audio = PlayerDecodedAudio::parse(
                    &decoded.format,
                    &decoded.bytes,
                    Self::MAX_DECODED_AUDIO_SAMPLES,
                )
                .map_err(|error| media_error("player.audio.contract", error))?;
                self.audio.start(source, executor, &request, audio).await?;
                tracing::info!(
                    event = "astra.player.audio.started",
                    command_id = %request.command_id,
                    command = %request.command,
                    asset_id = %request.asset_id,
                    encoded_hash = %request.encoded_hash,
                    decoded_hash = %decoded.hash,
                    "Player started packaged audio in the persistent mixer"
                );
            }

            self.audio
                .pump(source, executor, &mut self.completed_signals)
                .await?;
            if let Some(fence) = source.pending_wait().map(|wait| wait.fence.clone()) {
                if self.completed_signals.remove(&fence) {
                    let present = source
                        .complete_wait(fence)
                        .map_err(|error| media_error("player.media.complete_wait", error))?;
                    executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| media_error("player.media.present", error))?;
                    continue;
                }
            }
            return Ok(());
        }
        Err(media_error(
            "player.media.process",
            "ASTRA_PLAYER_MEDIA_COMPLETION_LOOP: completion chain exceeded its bound",
        ))
    }

    pub async fn shutdown(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.audio.shutdown(source, executor).await
    }
}

fn media_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
