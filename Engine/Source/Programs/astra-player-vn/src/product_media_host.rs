use std::collections::BTreeSet;

use astra_platform::{PlatformError, PlatformErrorCode};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerTimelineCompletion,
    PlayerTimelineScheduler, PlayerTimelineSchedulerSnapshot,
};

use crate::{
    NativeVnAudioOutput, NativeVnHostCommandSource, NativeVnProductAudioHost, NativeVnVideoRequest,
};

pub struct NativeVnProductMediaHost {
    audio: NativeVnProductAudioHost,
    timeline: PlayerTimelineScheduler,
    completed_signals: BTreeSet<String>,
    active_videos: Vec<ActiveVideoStream>,
    restored_videos: Vec<NativeVnVideoStreamSnapshot>,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
}

struct ActiveVideoStream {
    request: NativeVnVideoRequest,
    stream: astra_media::DecodedVideoStream,
    next_frame: usize,
    loop_index: u64,
    started_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnProductMediaSnapshot {
    pub schema: String,
    pub audio: crate::NativeVnProductAudioSnapshot,
    pub timeline: PlayerTimelineSchedulerSnapshot,
    pub completed_signals: Vec<String>,
    #[serde(default)]
    pub active_videos: Vec<NativeVnVideoStreamSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnVideoStreamSnapshot {
    pub layer: String,
    pub asset_id: String,
    pub encoded_hash: astra_core::Hash256,
    pub alpha_millionths: i64,
    #[serde(default)]
    pub looping: bool,
    pub fence: Option<String>,
    pub fallback_asset_id: Option<String>,
    pub allow_fallback: bool,
    pub duration_us: u64,
    pub next_frame: usize,
    #[serde(default)]
    pub loop_index: u64,
    pub started_at_ms: u64,
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
            active_videos: Vec::new(),
            restored_videos: Vec::new(),
            max_video_frames: 18_000,
            max_decode_output_bytes: 512 * 1024 * 1024,
        }
    }

    pub fn with_video_limits(
        max_timeline_tasks: usize,
        max_video_frames: u64,
        max_decode_output_bytes: u64,
    ) -> Result<Self, PlatformError> {
        if max_video_frames == 0 || max_decode_output_bytes == 0 {
            return Err(media_error(
                "player.media.create",
                "ASTRA_PLAYER_VIDEO_LIMIT_INVALID",
            ));
        }
        let mut host = Self::new(max_timeline_tasks);
        host.max_video_frames = max_video_frames;
        host.max_decode_output_bytes = max_decode_output_bytes;
        Ok(host)
    }

    pub fn is_active(&self) -> bool {
        self.timeline.active_count() > 0 || self.audio.is_active() || !self.active_videos.is_empty()
    }

    pub fn has_active_video(&self) -> bool {
        !self.active_videos.is_empty()
    }

    pub fn skip_active_videos(&mut self, source: &mut NativeVnHostCommandSource) -> bool {
        if self.active_videos.is_empty() {
            return false;
        }
        for video in &self.active_videos {
            source.complete_video_fence(&video.request);
            tracing::info!(
                event = "astra.player.video.skipped",
                asset_id = %video.request.asset_id,
                encoded_hash = %video.request.encoded_hash,
                "Player completed an active video fence from physical advance input"
            );
        }
        self.active_videos.clear();
        true
    }

    pub fn last_audio_meter(&self) -> Option<crate::NativeVnAudioMeterSnapshot> {
        self.audio.last_meter()
    }

    pub fn has_active_voice(&self) -> bool {
        self.audio.has_active_voice()
    }

    pub fn submitted_audio_timeline(&self) -> &[f32] {
        self.audio.submitted_timeline()
    }

    pub fn snapshot(&self) -> NativeVnProductMediaSnapshot {
        NativeVnProductMediaSnapshot {
            schema: "astra.player.native_vn_media_snapshot.v1".into(),
            audio: self.audio.snapshot(),
            timeline: self.timeline.snapshot(),
            completed_signals: self.completed_signals.iter().cloned().collect(),
            active_videos: self
                .active_videos
                .iter()
                .map(|video| NativeVnVideoStreamSnapshot {
                    layer: video.request.layer.clone(),
                    asset_id: video.request.asset_id.clone(),
                    encoded_hash: video.request.encoded_hash,
                    alpha_millionths: video.request.alpha_millionths,
                    looping: video.request.looping,
                    fence: video.request.fence.clone(),
                    fallback_asset_id: video.request.fallback_asset_id.clone(),
                    allow_fallback: video.request.allow_fallback,
                    duration_us: video.stream.duration_us,
                    next_frame: video.next_frame,
                    loop_index: video.loop_index,
                    started_at_ms: video.started_at_ms,
                })
                .collect(),
        }
    }

    pub fn restore(&mut self, snapshot: NativeVnProductMediaSnapshot) -> Result<(), PlatformError> {
        if snapshot.schema != "astra.player.native_vn_media_snapshot.v1" {
            return Err(media_error(
                "player.media.restore",
                "ASTRA_PLAYER_MEDIA_SNAPSHOT_INVALID",
            ));
        }
        let timeline = PlayerTimelineScheduler::restore(snapshot.timeline)
            .map_err(|error| media_error("player.media.timeline.restore", error))?;
        self.audio.restore(snapshot.audio)?;
        self.timeline = timeline;
        self.completed_signals = snapshot.completed_signals.into_iter().collect();
        self.active_videos.clear();
        self.restored_videos = snapshot.active_videos;
        Ok(())
    }

    pub async fn initialize(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.audio.ensure_open(source, executor).await
    }

    pub async fn poll_and_process(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
    ) -> Result<(), PlatformError> {
        self.poll_and_process_with_audio_tick(source, executor, now_ms, true)
            .await
    }

    pub async fn poll_and_process_with_audio_tick(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
        render_audio_tick: bool,
    ) -> Result<(), PlatformError> {
        let completed = self
            .timeline
            .poll(now_ms)
            .map_err(|error| media_error("player.timeline.poll", error))?;
        self.process_with_audio_tick(source, executor, now_ms, completed, render_audio_tick)
            .await
    }

    pub async fn process(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
        completed: Vec<PlayerTimelineCompletion>,
    ) -> Result<(), PlatformError> {
        self.process_with_audio_tick(source, executor, now_ms, completed, true)
            .await
    }

    pub async fn process_with_audio_tick(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
        mut completed: Vec<PlayerTimelineCompletion>,
        render_audio_tick: bool,
    ) -> Result<(), PlatformError> {
        self.restore_video_streams(source, executor).await?;
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

            for request in source.take_video_requests() {
                let decode = source
                    .prepare_video_decode(&request)
                    .map_err(|error| media_error("player.video.decode.prepare", error))?;
                match executor.execute_decode_lifecycle(decode).await {
                    Ok(decoded) => {
                        if decoded.format
                            == format!("postcard:{}", astra_media::DECODED_VIDEO_STREAM_SCHEMA)
                        {
                            let stream = astra_media::DecodedVideoStream::decode(
                                &decoded.bytes,
                                self.max_video_frames,
                                self.max_decode_output_bytes,
                            )
                            .map_err(|error| media_error("player.video.contract", error))?;
                            self.active_videos.push(ActiveVideoStream {
                                request: request.clone(),
                                stream,
                                next_frame: 0,
                                loop_index: 0,
                                started_at_ms: now_ms,
                            });
                        } else {
                            let frame = decoded_video_frame(&decoded.format, &decoded.bytes)?;
                            let present = source
                                .bind_decoded_video_frame(&request, frame, true)
                                .map_err(|error| media_error("player.video.bind", error))?;
                            executor
                                .execute_batch(present)
                                .await
                                .map_err(|error| media_error("player.video.present", error))?;
                        }
                        tracing::info!(
                            event = "astra.player.video.started",
                            asset_id = %request.asset_id,
                            encoded_hash = %request.encoded_hash,
                            decoded_hash = %decoded.hash,
                            "Player decoded and presented a packaged video frame"
                        );
                    }
                    Err(error) if request.allow_fallback => {
                        let present =
                            source
                                .bind_video_fallback(&request)
                                .map_err(|fallback_error| {
                                    media_error(
                                        "player.video.fallback",
                                        format!(
                                        "decode failed: {error}; fallback failed: {fallback_error}"
                                    ),
                                    )
                                })?;
                        executor
                            .execute_batch(present)
                            .await
                            .map_err(|fallback_error| {
                                media_error(
                                    "player.video.fallback.present",
                                    format!(
                                        "decode failed: {error}; fallback failed: {fallback_error}"
                                    ),
                                )
                            })?;
                        tracing::warn!(
                            event = "astra.player.video.authored_fallback",
                            asset_id = %request.asset_id,
                            fallback_asset_id = request.fallback_asset_id.as_deref().unwrap_or("missing"),
                            "Player used the package-authored video fallback after provider failure"
                        );
                    }
                    Err(error) => return Err(media_error("player.video.decode", error)),
                }
            }

            self.completed_signals
                .extend(source.take_stage_completions());

            self.present_due_video_frames(source, executor, now_ms)
                .await?;

            if render_audio_tick {
                self.audio
                    .pump(source, executor, &mut self.completed_signals)
                    .await?;
            }
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
        self.active_videos.clear();
        self.restored_videos.clear();
        self.audio.shutdown(source, executor).await
    }

    async fn restore_video_streams(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        for snapshot in std::mem::take(&mut self.restored_videos) {
            let request = source
                .rehydrate_video_request(&snapshot)
                .map_err(|error| media_error("player.video.restore.asset", error))?;
            let decode = source
                .prepare_video_decode(&request)
                .map_err(|error| media_error("player.video.restore.prepare", error))?;
            let decoded = executor
                .execute_decode_lifecycle(decode)
                .await
                .map_err(|error| media_error("player.video.restore.decode", error))?;
            if decoded.format != format!("postcard:{}", astra_media::DECODED_VIDEO_STREAM_SCHEMA) {
                return Err(media_error(
                    "player.video.restore.contract",
                    "ASTRA_PLAYER_VIDEO_STREAM_REQUIRED",
                ));
            }
            let stream = astra_media::DecodedVideoStream::decode(
                &decoded.bytes,
                self.max_video_frames,
                self.max_decode_output_bytes,
            )
            .map_err(|error| media_error("player.video.restore.contract", error))?;
            if stream.duration_us != snapshot.duration_us
                || snapshot.next_frame > stream.frames.len()
            {
                return Err(media_error(
                    "player.video.restore.identity",
                    "ASTRA_PLAYER_VIDEO_STREAM_IDENTITY_MISMATCH",
                ));
            }
            self.active_videos.push(ActiveVideoStream {
                request,
                stream,
                next_frame: snapshot.next_frame,
                loop_index: snapshot.loop_index,
                started_at_ms: snapshot.started_at_ms,
            });
        }
        Ok(())
    }

    async fn present_due_video_frames(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
    ) -> Result<(), PlatformError> {
        let mut completed = Vec::new();
        for (index, video) in self.active_videos.iter_mut().enumerate() {
            let elapsed_us = now_ms
                .saturating_sub(video.started_at_ms)
                .saturating_mul(1_000);
            loop {
                while let Some(frame) = video.stream.frames.get(video.next_frame) {
                    let loop_offset = video
                        .loop_index
                        .checked_mul(video.stream.duration_us)
                        .ok_or_else(|| {
                            media_error(
                                "player.video.stream.clock",
                                "ASTRA_PLAYER_VIDEO_LOOP_TIME_OVERFLOW",
                            )
                        })?;
                    let due_us = loop_offset.checked_add(frame.pts_us).ok_or_else(|| {
                        media_error(
                            "player.video.stream.clock",
                            "ASTRA_PLAYER_VIDEO_FRAME_TIME_OVERFLOW",
                        )
                    })?;
                    if due_us > elapsed_us {
                        break;
                    }
                    let texture = decoded_bgra_frame(
                        frame.width,
                        frame.height,
                        frame.content_hash,
                        &frame.bgra8,
                    )?;
                    video.next_frame += 1;
                    let present = source
                        .bind_decoded_video_frame(&video.request, texture, false)
                        .map_err(|error| media_error("player.video.stream.bind", error))?;
                    executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| media_error("player.video.stream.present", error))?;
                }
                if video.next_frame != video.stream.frames.len() {
                    break;
                }
                let loop_end_us = video
                    .loop_index
                    .checked_add(1)
                    .and_then(|loop_index| loop_index.checked_mul(video.stream.duration_us))
                    .ok_or_else(|| {
                        media_error(
                            "player.video.stream.clock",
                            "ASTRA_PLAYER_VIDEO_LOOP_TIME_OVERFLOW",
                        )
                    })?;
                if elapsed_us < loop_end_us {
                    break;
                }
                if video.request.looping {
                    video.loop_index = video.loop_index.checked_add(1).ok_or_else(|| {
                        media_error(
                            "player.video.stream.clock",
                            "ASTRA_PLAYER_VIDEO_LOOP_INDEX_OVERFLOW",
                        )
                    })?;
                    video.next_frame = 0;
                    continue;
                }
                if video.request.fence.is_some() {
                    source.complete_video_fence(&video.request);
                }
                completed.push(index);
                break;
            }
        }
        for index in completed.into_iter().rev() {
            self.active_videos.remove(index);
        }
        Ok(())
    }
}

fn decoded_video_frame(
    format: &str,
    bytes: &[u8],
) -> Result<astra_media_core::TextureFrame, PlatformError> {
    let dimensions = format
        .strip_prefix("bgra8:first_frame:")
        .and_then(|value| value.split_once('x'))
        .and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
        .ok_or_else(|| media_error("player.video.contract", "ASTRA_PLAYER_VIDEO_FORMAT"))?;
    let expected = usize::try_from(dimensions.0)
        .ok()
        .and_then(|width| {
            usize::try_from(dimensions.1)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| media_error("player.video.contract", "ASTRA_PLAYER_VIDEO_SIZE"))?;
    if bytes.len() != expected || expected == 0 {
        return Err(media_error(
            "player.video.contract",
            "ASTRA_PLAYER_VIDEO_BUFFER_SIZE",
        ));
    }
    decoded_bgra_frame(
        dimensions.0,
        dimensions.1,
        astra_core::Hash256::from_sha256(bytes),
        bytes,
    )
}

fn decoded_bgra_frame(
    width: u32,
    height: u32,
    expected_hash: astra_core::Hash256,
    bytes: &[u8],
) -> Result<astra_media_core::TextureFrame, PlatformError> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| media_error("player.video.contract", "ASTRA_PLAYER_VIDEO_SIZE"))?;
    if bytes.len() != expected
        || expected == 0
        || astra_core::Hash256::from_sha256(bytes) != expected_hash
    {
        return Err(media_error(
            "player.video.contract",
            "ASTRA_PLAYER_VIDEO_BUFFER_SIZE_OR_HASH",
        ));
    }
    let mut rgba8 = bytes.to_vec();
    for pixel in rgba8.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(astra_media_core::TextureFrame {
        width,
        height,
        hash: astra_core::Hash256::from_sha256(&rgba8),
        rgba8,
    })
}

fn media_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
