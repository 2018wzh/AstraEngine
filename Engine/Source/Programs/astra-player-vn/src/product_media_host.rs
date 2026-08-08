use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};

use astra_media::{PcmAsset, CANONICAL_CHANNELS, CANONICAL_SAMPLE_RATE};
use astra_platform::{PlatformError, PlatformErrorCode};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerTimelineCompletion,
    PlayerTimelineScheduler, PlayerTimelineSchedulerSnapshot,
};

use crate::{
    NativeVnAudioOutput, NativeVnAudioPreloadRequest, NativeVnDecodedCacheBudget,
    NativeVnHostCommandSource, NativeVnHostError, NativeVnProductAudioHost, NativeVnVideoRequest,
    DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
};

pub struct NativeVnProductMediaHost {
    audio: NativeVnProductAudioHost,
    timeline: PlayerTimelineScheduler,
    completed_signals: BTreeSet<String>,
    active_videos: Vec<ActiveVideoStream>,
    pending_video_closes: Vec<astra_player_core::PlayerHostResourceId>,
    restored_videos: Vec<NativeVnVideoStreamSnapshot>,
    decoded_audio_cache: BTreeMap<AudioCacheKey, CachedDecodedAudio>,
    decoded_audio_lru: VecDeque<(AudioCacheKey, u64)>,
    decoded_audio_access_epoch: u64,
    decoded_audio_cache_bytes: u64,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
    max_decoded_cache_bytes: u64,
    performance: Option<NativeVnMediaPerformanceSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeVnMediaPerformanceSample {
    pub prewarm_ns: u64,
    pub provider_decode_ns: u64,
    pub parse_convert_ns: u64,
    pub mixer_ns: u64,
    pub audio_query_ns: u64,
    pub audio_render_ns: u64,
    pub audio_submit_ns: u64,
    pub audio_completion_ns: u64,
}

#[derive(Clone)]
struct CachedDecodedAudio {
    asset: PcmAsset,
    byte_size: u64,
    last_access_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AudioCacheKey {
    asset_id: String,
    codec: String,
    encoded_length: u64,
    target_sample_rate: u32,
    target_channels: u16,
}

impl AudioCacheKey {
    fn new(asset_id: &str, codec: &str, encoded_length: u64) -> Self {
        Self {
            asset_id: asset_id.to_string(),
            codec: codec.to_string(),
            encoded_length,
            target_sample_rate: CANONICAL_SAMPLE_RATE,
            target_channels: CANONICAL_CHANNELS,
        }
    }
}

struct ActiveVideoStream {
    request: NativeVnVideoRequest,
    session: astra_player_core::PlayerHostResourceId,
    duration_us: u64,
    expected_frame_count: Option<u64>,
    expected_decoded_byte_count: Option<u64>,
    decoded_byte_count: u64,
    pending_frame: Option<astra_media::DecodedVideoFrame>,
    next_frame: u64,
    next_request_sequence: u64,
    reached_end: bool,
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
    pub encoded_length: u64,
    pub alpha_millionths: i64,
    #[serde(default)]
    pub looping: bool,
    pub fence: Option<String>,
    pub duration_us: u64,
    pub decoded_byte_count: u64,
    pub next_frame: u64,
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
        Self::with_audio_timeline_retention(max_timeline_tasks, true)
    }

    pub fn with_audio_timeline_retention(
        max_timeline_tasks: usize,
        retain_audio_timeline: bool,
    ) -> Self {
        let cache_budget =
            NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES)
                .expect("the built-in NativeVN decoded-cache budget must be partitionable");
        Self {
            audio: NativeVnProductAudioHost::new(retain_audio_timeline),
            timeline: PlayerTimelineScheduler::new(max_timeline_tasks),
            completed_signals: BTreeSet::new(),
            active_videos: Vec::new(),
            pending_video_closes: Vec::new(),
            restored_videos: Vec::new(),
            decoded_audio_cache: BTreeMap::new(),
            decoded_audio_lru: VecDeque::new(),
            decoded_audio_access_epoch: 0,
            decoded_audio_cache_bytes: 0,
            max_video_frames: 18_000,
            max_decode_output_bytes: 512 * 1024 * 1024,
            max_decoded_cache_bytes: cache_budget.audio_bytes,
            performance: None,
        }
    }

    pub fn with_video_limits(
        max_timeline_tasks: usize,
        max_video_frames: u64,
        max_decode_output_bytes: u64,
        max_decoded_cache_bytes: u64,
        retain_audio_timeline: bool,
    ) -> Result<Self, PlatformError> {
        if max_video_frames == 0
            || max_decode_output_bytes == 0
            || max_decoded_cache_bytes == 0
            || max_decoded_cache_bytes > max_decode_output_bytes
        {
            return Err(media_error(
                "player.media.create",
                "ASTRA_PLAYER_VIDEO_LIMIT_INVALID",
            ));
        }
        let mut host =
            Self::with_audio_timeline_retention(max_timeline_tasks, retain_audio_timeline);
        host.max_video_frames = max_video_frames;
        host.max_decode_output_bytes = max_decode_output_bytes;
        host.max_decoded_cache_bytes = max_decoded_cache_bytes;
        Ok(host)
    }

    pub fn set_performance_profiling(&mut self, enabled: bool) {
        self.performance = enabled.then(NativeVnMediaPerformanceSample::default);
    }

    pub fn take_performance_sample(&mut self) -> NativeVnMediaPerformanceSample {
        self.performance
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    pub fn is_active(&self) -> bool {
        self.timeline.active_count() > 0 || self.audio.is_active() || !self.active_videos.is_empty()
    }

    pub async fn recover_audio_device_loss(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.audio.recover_device_loss(source, executor).await
    }

    pub fn has_active_video(&self) -> bool {
        !self.active_videos.is_empty()
    }

    pub fn decoded_cache_bytes(&self) -> u64 {
        self.decoded_audio_cache_bytes
    }

    pub fn skip_active_videos(
        &mut self,
        source: &mut NativeVnHostCommandSource,
    ) -> Result<bool, NativeVnHostError> {
        if self.active_videos.is_empty() {
            return Ok(false);
        }
        for video in &self.active_videos {
            source.complete_video_fence(&video.request)?;
            self.pending_video_closes.push(video.session);
            tracing::info!(
                event = "astra.player.video.skipped",
                asset_id = %video.request.asset_id,
                encoded_length = video.request.encoded_length,
                "Player completed an active video fence from physical advance input"
            );
        }
        self.active_videos.clear();
        Ok(true)
    }

    pub fn last_audio_meter(&self) -> Option<crate::NativeVnAudioMeterSnapshot> {
        self.audio.last_meter()
    }

    pub fn has_active_voice(&self) -> bool {
        self.audio.has_active_voice()
    }

    pub fn submitted_audio_timeline(&self) -> Result<Vec<f32>, PlatformError> {
        self.audio.submitted_timeline()
    }

    pub fn snapshot(&self) -> NativeVnProductMediaSnapshot {
        NativeVnProductMediaSnapshot {
            schema: "astra.player.native_vn_media_snapshot.v2".into(),
            audio: self.audio.snapshot(),
            timeline: self.timeline.snapshot(),
            completed_signals: self.completed_signals.iter().cloned().collect(),
            active_videos: self
                .active_videos
                .iter()
                .map(|video| NativeVnVideoStreamSnapshot {
                    layer: video.request.layer.clone(),
                    asset_id: video.request.asset_id.clone(),
                    encoded_length: video.request.encoded_length,
                    alpha_millionths: video.request.alpha_millionths,
                    looping: video.request.looping,
                    fence: video.request.fence.clone(),
                    duration_us: video.duration_us,
                    decoded_byte_count: video
                        .decoded_byte_count
                        .checked_sub(
                            video
                                .pending_frame
                                .as_ref()
                                .map_or(0, |frame| frame.bgra8.len() as u64),
                        )
                        .expect("pending video bytes are included in decoded byte accounting"),
                    next_frame: video.next_frame,
                    loop_index: video.loop_index,
                    started_at_ms: video.started_at_ms,
                })
                .collect(),
        }
    }

    pub fn restore(&mut self, snapshot: NativeVnProductMediaSnapshot) -> Result<(), PlatformError> {
        if snapshot.schema != "astra.player.native_vn_media_snapshot.v2" {
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
        self.pending_video_closes.clear();
        self.restored_videos = snapshot.active_videos;
        Ok(())
    }

    pub async fn initialize(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.audio.ensure_open(source, executor).await?;
        self.prewarm_pending_audio(source, executor).await
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
        self.close_pending_video_streams(source, executor).await?;
        let prewarm_started = self.performance.as_ref().map(|_| Instant::now());
        self.prewarm_pending_audio(source, executor).await?;
        self.add_profile_duration(prewarm_started, |sample, duration| {
            sample.prewarm_ns = sample.prewarm_ns.saturating_add(duration);
        })?;
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
                tracing::debug!(
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
                let cache_key =
                    AudioCacheKey::new(&request.asset_id, &request.codec, request.encoded_length);
                let (audio, cache_hit) = if let Some(cached) = self.cached_audio(&cache_key) {
                    (cached.asset.with_identity(request.asset_id.clone()), true)
                } else {
                    let cached = self
                        .decode_audio(
                            source,
                            executor,
                            &NativeVnAudioPreloadRequest {
                                asset_id: request.asset_id.clone(),
                                codec: request.codec.clone(),
                                encoded_bytes: request.encoded_bytes.clone(),
                                encoded_length: request.encoded_length,
                            },
                        )
                        .await?;
                    (cached.asset.with_identity(request.asset_id.clone()), false)
                };
                let mixer_started = self.performance.as_ref().map(|_| Instant::now());
                self.audio
                    .start_canonical(
                        source,
                        executor,
                        &request,
                        audio,
                        &mut self.completed_signals,
                    )
                    .await?;
                self.add_profile_duration(mixer_started, |sample, duration| {
                    sample.mixer_ns = sample.mixer_ns.saturating_add(duration);
                })?;
                tracing::trace!(
                    event = "astra.player.audio.started",
                    command_id = %request.command_id,
                    command = %request.command,
                    asset_id = %request.asset_id,
                    encoded_length = request.encoded_length,
                    cache_hit,
                    "Player submitted packaged audio to the Kira service"
                );
            }

            for request in source.take_video_requests() {
                let decode_lease = astra_plugin::WorkerBudgetBroker::global()
                    .acquire()
                    .await
                    .map_err(|error| media_error("player.video.decode.budget", error))?;
                match self
                    .open_video_stream(source, executor, request.clone(), now_ms)
                    .await
                {
                    Ok(video) => {
                        drop(decode_lease);
                        self.active_videos.push(video);
                        tracing::info!(
                            event = "astra.player.video.started",
                            asset_id = %request.asset_id,
                            encoded_length = request.encoded_length,
                            "Player opened a bounded packaged video stream"
                        );
                    }
                    Err(error) => {
                        drop(decode_lease);
                        return Err(media_error("player.video.decode", error));
                    }
                }
            }

            self.completed_signals
                .extend(source.take_stage_completions());

            self.present_due_video_frames(source, executor, now_ms)
                .await?;

            if render_audio_tick {
                let mixer_started = self.performance.as_ref().map(|_| Instant::now());
                let audio_sample = self
                    .audio
                    .pump(
                        source,
                        executor,
                        &mut self.completed_signals,
                        self.performance.is_some(),
                    )
                    .await?;
                self.add_profile_duration(mixer_started, |sample, duration| {
                    sample.mixer_ns = sample.mixer_ns.saturating_add(duration);
                    sample.audio_query_ns =
                        sample.audio_query_ns.saturating_add(audio_sample.query_ns);
                    sample.audio_render_ns = sample
                        .audio_render_ns
                        .saturating_add(audio_sample.render_ns);
                    sample.audio_submit_ns = sample
                        .audio_submit_ns
                        .saturating_add(audio_sample.submit_ns);
                    sample.audio_completion_ns = sample
                        .audio_completion_ns
                        .saturating_add(audio_sample.completion_ns);
                })?;
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

    fn add_profile_duration(
        &mut self,
        started: Option<Instant>,
        update: impl FnOnce(&mut NativeVnMediaPerformanceSample, u64),
    ) -> Result<(), PlatformError> {
        let Some(started) = started else {
            return Ok(());
        };
        let elapsed = started.elapsed().as_nanos();
        let duration = u64::try_from(elapsed).map_err(|_| {
            media_error(
                "player.media.performance",
                "ASTRA_PLAYER_MEDIA_PERFORMANCE_DURATION_OVERFLOW",
            )
        })?;
        let sample = self.performance.as_mut().ok_or_else(|| {
            media_error(
                "player.media.performance",
                "ASTRA_PLAYER_MEDIA_PERFORMANCE_STATE_MISSING",
            )
        })?;
        update(sample, duration);
        Ok(())
    }

    pub async fn shutdown(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.pending_video_closes
            .extend(self.active_videos.iter().map(|video| video.session));
        self.active_videos.clear();
        self.restored_videos.clear();
        self.close_pending_video_streams(source, executor).await?;
        self.audio.shutdown(source, executor).await
    }

    async fn open_video_stream(
        &self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: NativeVnVideoRequest,
        started_at_ms: u64,
    ) -> Result<ActiveVideoStream, PlatformError> {
        let plan = source
            .prepare_video_decode(&request)
            .map_err(|error| media_error("player.video.decode.prepare", error))?;
        executor
            .execute_decode_open(plan.session, plan.open)
            .await
            .map_err(|error| media_error("player.video.decode.open", error))?;
        let decoded = match executor
            .execute_decode_submit(plan.session, plan.decode)
            .await
        {
            Ok(decoded) => decoded,
            Err(error) => {
                let close =
                    source
                        .prepare_video_stream_close(plan.session)
                        .map_err(|close_error| {
                            media_error(
                                "player.video.decode.cleanup.prepare",
                                format!("{error}; close preparation failed: {close_error}"),
                            )
                        })?;
                if let Err(close_error) = executor.execute_decode_close(plan.session, close).await {
                    return Err(media_error(
                        "player.video.decode.cleanup",
                        format!("{error}; close failed: {close_error}"),
                    ));
                }
                return Err(media_error("player.video.decode.start", error));
            }
        };
        let astra_platform::DecodeOutput::VideoStreamStart {
            duration_us: Some(duration_us),
            frame_count,
            decoded_byte_count,
        } = decoded.output
        else {
            let close = source
                .prepare_video_stream_close(plan.session)
                .map_err(|error| media_error("player.video.decode.cleanup.prepare", error))?;
            executor
                .execute_decode_close(plan.session, close)
                .await
                .map_err(|error| media_error("player.video.decode.cleanup", error))?;
            return Err(media_error(
                "player.video.decode.contract",
                "ASTRA_PLAYER_VIDEO_STREAM_DESCRIPTOR_REQUIRED",
            ));
        };
        if duration_us == 0
            || frame_count.is_some_and(|count| count == 0 || count > self.max_video_frames)
            || decoded_byte_count
                .is_some_and(|bytes| bytes == 0 || bytes > self.max_decode_output_bytes)
        {
            return Err(media_error(
                "player.video.decode.contract",
                "ASTRA_PLAYER_VIDEO_STREAM_DESCRIPTOR_INVALID",
            ));
        }
        Ok(ActiveVideoStream {
            request,
            session: plan.session,
            duration_us,
            expected_frame_count: frame_count,
            expected_decoded_byte_count: decoded_byte_count,
            decoded_byte_count: 0,
            pending_frame: None,
            next_frame: 0,
            next_request_sequence: 2,
            reached_end: false,
            loop_index: 0,
            started_at_ms,
        })
    }

    async fn close_pending_video_streams(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        for session in std::mem::take(&mut self.pending_video_closes) {
            let close = source
                .prepare_video_stream_close(session)
                .map_err(|error| media_error("player.video.close.prepare", error))?;
            executor
                .execute_decode_close(session, close)
                .await
                .map_err(|error| media_error("player.video.close", error))?;
        }
        Ok(())
    }

    async fn fetch_video_frame(
        max_decode_output_bytes: u64,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        video: &mut ActiveVideoStream,
    ) -> Result<Option<astra_media::DecodedVideoFrame>, PlatformError> {
        if video.reached_end {
            return Ok(None);
        }
        let next = source
            .prepare_video_stream_next(video.session, video.next_request_sequence)
            .map_err(|error| media_error("player.video.stream.next.prepare", error))?;
        let decoded = executor
            .execute_decode_submit(video.session, next)
            .await
            .map_err(|error| media_error("player.video.stream.next", error))?;
        video.next_request_sequence =
            video.next_request_sequence.checked_add(1).ok_or_else(|| {
                media_error(
                    "player.video.stream.next",
                    "ASTRA_PLAYER_VIDEO_REQUEST_SEQUENCE_OVERFLOW",
                )
            })?;
        match decoded.output {
            astra_platform::DecodeOutput::VideoFrame {
                sequence,
                pts_us,
                duration_us,
                width,
                height,
                bgra8,
            } => {
                let frame = astra_media::DecodedVideoFrame {
                    sequence,
                    pts_us,
                    duration_us,
                    width,
                    height,
                    bgra8,
                };
                frame
                    .validate()
                    .map_err(|error| media_error("player.video.stream.frame", error))?;
                let expected_sequence = video.next_frame.checked_add(1).ok_or_else(|| {
                    media_error(
                        "player.video.stream.frame",
                        "ASTRA_PLAYER_VIDEO_FRAME_SEQUENCE_OVERFLOW",
                    )
                })?;
                if frame.sequence != expected_sequence
                    || frame.bgra8.len() as u64 > max_decode_output_bytes
                    || frame.pts_us >= video.duration_us
                    || frame
                        .pts_us
                        .checked_add(frame.duration_us)
                        .is_none_or(|end| end > video.duration_us)
                {
                    return Err(media_error(
                        "player.video.stream.frame",
                        "ASTRA_PLAYER_VIDEO_FRAME_IDENTITY_MISMATCH",
                    ));
                }
                video.decoded_byte_count = video
                    .decoded_byte_count
                    .checked_add(frame.bgra8.len() as u64)
                    .ok_or_else(|| {
                        media_error(
                            "player.video.stream.frame",
                            "ASTRA_PLAYER_VIDEO_DECODED_BYTES_OVERFLOW",
                        )
                    })?;
                if video.decoded_byte_count > max_decode_output_bytes {
                    return Err(media_error(
                        "player.video.stream.frame",
                        "ASTRA_PLAYER_VIDEO_DECODED_BYTES_BUDGET",
                    ));
                }
                Ok(Some(frame))
            }
            astra_platform::DecodeOutput::VideoStreamEnd {
                frame_count,
                decoded_byte_count,
            } => {
                if video.next_frame != frame_count
                    || video.decoded_byte_count != decoded_byte_count
                    || video
                        .expected_frame_count
                        .is_some_and(|count| count != frame_count)
                    || video
                        .expected_decoded_byte_count
                        .is_some_and(|bytes| bytes != decoded_byte_count)
                {
                    return Err(media_error(
                        "player.video.stream.end",
                        "ASTRA_PLAYER_VIDEO_STREAM_ENDED_EARLY",
                    ));
                }
                video.reached_end = true;
                Ok(None)
            }
            _ => Err(media_error(
                "player.video.stream.next",
                "ASTRA_PLAYER_VIDEO_STREAM_OUTPUT_INVALID",
            )),
        }
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
            let decode_lease = astra_plugin::WorkerBudgetBroker::global()
                .acquire()
                .await
                .map_err(|error| media_error("player.video.restore.budget", error))?;
            let mut video = self
                .open_video_stream(source, executor, request, snapshot.started_at_ms)
                .await
                .map_err(|error| media_error("player.video.restore.decode", error))?;
            drop(decode_lease);
            if video.duration_us != snapshot.duration_us {
                self.pending_video_closes.push(video.session);
                return Err(media_error(
                    "player.video.restore.identity",
                    "ASTRA_PLAYER_VIDEO_STREAM_IDENTITY_MISMATCH",
                ));
            }
            for expected in 1..=snapshot.next_frame {
                let frame = Self::fetch_video_frame(
                    self.max_decode_output_bytes,
                    source,
                    executor,
                    &mut video,
                )
                .await?
                .ok_or_else(|| {
                    media_error(
                        "player.video.restore.cursor",
                        "ASTRA_PLAYER_VIDEO_RESTORE_CURSOR_OUT_OF_RANGE",
                    )
                })?;
                if frame.sequence != expected {
                    self.pending_video_closes.push(video.session);
                    return Err(media_error(
                        "player.video.restore.cursor",
                        "ASTRA_PLAYER_VIDEO_RESTORE_SEQUENCE_MISMATCH",
                    ));
                }
                video.next_frame = expected;
            }
            if video.decoded_byte_count != snapshot.decoded_byte_count {
                self.pending_video_closes.push(video.session);
                return Err(media_error(
                    "player.video.restore.identity",
                    "ASTRA_PLAYER_VIDEO_STREAM_IDENTITY_MISMATCH",
                ));
            }
            video.loop_index = snapshot.loop_index;
            self.active_videos.push(video);
        }
        Ok(())
    }

    fn cached_audio(&mut self, key: &AudioCacheKey) -> Option<CachedDecodedAudio> {
        if !self.touch_cached_audio(key) {
            return None;
        }
        self.decoded_audio_cache.get(key).cloned()
    }

    fn touch_cached_audio(&mut self, key: &AudioCacheKey) -> bool {
        self.compact_audio_recency_if_needed();
        if !self.decoded_audio_cache.contains_key(key) {
            return false;
        }
        self.decoded_audio_access_epoch = self.decoded_audio_access_epoch.saturating_add(1);
        let epoch = self.decoded_audio_access_epoch;
        self.decoded_audio_cache
            .get_mut(key)
            .expect("cache membership was checked")
            .last_access_epoch = epoch;
        self.decoded_audio_lru.push_back((key.clone(), epoch));
        true
    }

    async fn prewarm_pending_audio(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        let requests = source.take_audio_preload_requests();
        if requests.is_empty() {
            return Ok(());
        }
        let requested_count = requests
            .iter()
            .map(|request| {
                AudioCacheKey::new(&request.asset_id, &request.codec, request.encoded_length)
            })
            .collect::<BTreeSet<_>>()
            .len();
        let mut required_keys = BTreeSet::new();
        for request in &requests {
            let key = AudioCacheKey::new(&request.asset_id, &request.codec, request.encoded_length);
            required_keys.insert(key.clone());
            if !self.touch_cached_audio(&key) {
                let cached = self
                    .decode_audio_uncached(source, executor, request)
                    .await?;
                if !self.cache_audio_without_eviction(key, cached) {
                    break;
                }
            }
        }
        let retained_count = required_keys
            .iter()
            .filter(|key| self.decoded_audio_cache.contains_key(*key))
            .count();
        tracing::info!(
            event = "astra.player.audio.prewarm.completed",
            requested_count,
            retained_count,
            decoded_cache_bytes = self.decoded_audio_cache_bytes,
            cache_budget_bytes = self.max_decoded_cache_bytes,
            "prewarmed an authored-order audio prefix within the bounded decoded cache"
        );
        Ok(())
    }

    async fn decode_audio(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: &NativeVnAudioPreloadRequest,
    ) -> Result<CachedDecodedAudio, PlatformError> {
        let cached = self
            .decode_audio_uncached(source, executor, request)
            .await?;
        self.cache_audio(
            AudioCacheKey::new(&request.asset_id, &request.codec, request.encoded_length),
            cached.asset.clone(),
        );
        Ok(cached)
    }

    async fn decode_audio_uncached(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: &NativeVnAudioPreloadRequest,
    ) -> Result<CachedDecodedAudio, PlatformError> {
        let decode_started = self.performance.as_ref().map(|_| Instant::now());
        let decode = source
            .prepare_audio_preload_decode(request)
            .map_err(|error| media_error("player.audio.decode.prepare", error))?;
        let decode_lease = astra_plugin::WorkerBudgetBroker::global()
            .acquire()
            .await
            .map_err(|error| media_error("player.audio.decode.budget", error))?;
        let decoded = executor
            .execute_decode_lifecycle(decode)
            .await
            .map_err(|error| media_error("player.audio.decode", error))?;
        drop(decode_lease);
        self.add_profile_duration(decode_started, |sample, duration| {
            sample.provider_decode_ns = sample.provider_decode_ns.saturating_add(duration);
        })?;
        let convert_started = self.performance.as_ref().map(|_| Instant::now());
        let audio = match decoded.output {
            astra_platform::DecodeOutput::AudioPcmI16 {
                sample_rate,
                channels,
                samples,
            } => PlayerDecodedAudio::from_i16(
                sample_rate,
                channels,
                samples,
                Self::MAX_DECODED_AUDIO_SAMPLES,
            ),
            astra_platform::DecodeOutput::AudioPcmF32 {
                sample_rate,
                channels,
                samples,
            } => PlayerDecodedAudio::from_f32(
                sample_rate,
                channels,
                samples,
                Self::MAX_DECODED_AUDIO_SAMPLES,
            ),
            _ => {
                return Err(media_error(
                    "player.audio.contract",
                    "ASTRA_PLAYER_AUDIO_TYPED_PCM_REQUIRED",
                ));
            }
        }
        .map_err(|error| media_error("player.audio.contract", error))?
        .into_converted(
            CANONICAL_SAMPLE_RATE,
            CANONICAL_CHANNELS,
            crate::NativeVnProductAudioHost::MAX_CONVERTED_SAMPLES,
        )
        .map_err(|error| media_error("player.audio.convert", error))?;
        let asset = PcmAsset::from_canonical_samples(request.asset_id.clone(), audio.samples)
            .map_err(|error| media_error("player.audio.asset", error))?;
        self.add_profile_duration(convert_started, |sample, duration| {
            sample.parse_convert_ns = sample.parse_convert_ns.saturating_add(duration);
        })?;
        Ok(CachedDecodedAudio {
            byte_size: (asset.samples.len() as u64)
                .saturating_mul(std::mem::size_of::<f32>() as u64),
            asset,
            last_access_epoch: 0,
        })
    }

    fn cache_audio_without_eviction(
        &mut self,
        key: AudioCacheKey,
        mut cached: CachedDecodedAudio,
    ) -> bool {
        if cached.byte_size > self.max_decoded_cache_bytes
            || self
                .decoded_audio_cache_bytes
                .saturating_add(cached.byte_size)
                > self.max_decoded_cache_bytes
        {
            return false;
        }
        self.compact_audio_recency_if_needed();
        self.decoded_audio_access_epoch = self.decoded_audio_access_epoch.saturating_add(1);
        cached.last_access_epoch = self.decoded_audio_access_epoch;
        self.decoded_audio_cache_bytes = self
            .decoded_audio_cache_bytes
            .saturating_add(cached.byte_size);
        self.decoded_audio_lru
            .push_back((key.clone(), cached.last_access_epoch));
        self.decoded_audio_cache.insert(key, cached);
        true
    }

    fn cache_audio(&mut self, key: AudioCacheKey, asset: PcmAsset) {
        let byte_size =
            (asset.samples.len() as u64).saturating_mul(std::mem::size_of::<f32>() as u64);
        if byte_size > self.max_decoded_cache_bytes {
            tracing::debug!(
                event = "astra.player.audio.cache.bypass",
                asset_id = %key.asset_id,
                encoded_length = key.encoded_length,
                byte_size,
                cache_budget_bytes = self.max_decoded_cache_bytes,
                "decoded audio exceeded the bounded session cache budget"
            );
            return;
        }
        if let Some(previous) = self.decoded_audio_cache.remove(&key) {
            self.decoded_audio_cache_bytes = self
                .decoded_audio_cache_bytes
                .saturating_sub(previous.byte_size);
        }
        while self.decoded_audio_cache_bytes.saturating_add(byte_size)
            > self.max_decoded_cache_bytes
        {
            let Some((evicted_key, access_epoch)) = self.decoded_audio_lru.pop_front() else {
                break;
            };
            if self
                .decoded_audio_cache
                .get(&evicted_key)
                .is_some_and(|cached| cached.last_access_epoch != access_epoch)
            {
                continue;
            }
            if let Some(evicted) = self.decoded_audio_cache.remove(&evicted_key) {
                self.decoded_audio_cache_bytes = self
                    .decoded_audio_cache_bytes
                    .saturating_sub(evicted.byte_size);
                tracing::debug!(
                    event = "astra.player.audio.cache.evicted",
                    asset_id = %evicted_key.asset_id,
                    encoded_length = evicted_key.encoded_length,
                    byte_size = evicted.byte_size,
                    "evicted least-recently-used decoded audio from the session cache"
                );
            }
        }
        self.compact_audio_recency_if_needed();
        self.decoded_audio_access_epoch = self.decoded_audio_access_epoch.saturating_add(1);
        let access_epoch = self.decoded_audio_access_epoch;
        self.decoded_audio_cache.insert(
            key.clone(),
            CachedDecodedAudio {
                asset,
                byte_size,
                last_access_epoch: access_epoch,
            },
        );
        self.decoded_audio_lru.push_back((key, access_epoch));
        self.decoded_audio_cache_bytes = self.decoded_audio_cache_bytes.saturating_add(byte_size);
    }

    fn compact_audio_recency_if_needed(&mut self) {
        let maximum_entries = self
            .decoded_audio_cache
            .len()
            .saturating_mul(4)
            .saturating_add(32);
        if self.decoded_audio_access_epoch != u64::MAX
            && self.decoded_audio_lru.len() <= maximum_entries
        {
            return;
        }
        let mut recency = self
            .decoded_audio_cache
            .iter()
            .map(|(key, cached)| (key.clone(), cached.last_access_epoch))
            .collect::<Vec<_>>();
        recency.sort_unstable_by_key(|(_, epoch)| *epoch);
        self.decoded_audio_lru.clear();
        for (index, (hash, _)) in recency.into_iter().enumerate() {
            let epoch = index as u64 + 1;
            if let Some(cached) = self.decoded_audio_cache.get_mut(&hash) {
                cached.last_access_epoch = epoch;
            }
            self.decoded_audio_lru.push_back((hash, epoch));
            self.decoded_audio_access_epoch = epoch;
        }
        if self.decoded_audio_cache.is_empty() {
            self.decoded_audio_access_epoch = 0;
        }
    }

    async fn present_due_video_frames(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
    ) -> Result<(), PlatformError> {
        let mut completed = Vec::new();
        let mut restarts = Vec::new();
        for (index, video) in self.active_videos.iter_mut().enumerate() {
            let elapsed_us = now_ms
                .saturating_sub(video.started_at_ms)
                .saturating_mul(1_000);
            loop {
                if video.pending_frame.is_none() && !video.reached_end {
                    video.pending_frame = Self::fetch_video_frame(
                        self.max_decode_output_bytes,
                        source,
                        executor,
                        video,
                    )
                    .await?;
                }
                if let Some(frame) = video.pending_frame.as_ref() {
                    let loop_offset =
                        video
                            .loop_index
                            .checked_mul(video.duration_us)
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
                    let frame = video
                        .pending_frame
                        .take()
                        .expect("pending frame was checked");
                    let texture = decoded_bgra_frame(frame.width, frame.height, frame.bgra8)?;
                    video.next_frame = frame.sequence;
                    let present = source
                        .bind_decoded_video_frame(&video.request, texture, false)
                        .map_err(|error| media_error("player.video.stream.bind", error))?;
                    executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| media_error("player.video.stream.present", error))?;
                    continue;
                }
                if !video.reached_end {
                    break;
                }
                let loop_end_us = video
                    .loop_index
                    .checked_add(1)
                    .and_then(|loop_index| loop_index.checked_mul(video.duration_us))
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
                    let loop_index = video.loop_index.checked_add(1).ok_or_else(|| {
                        media_error(
                            "player.video.stream.clock",
                            "ASTRA_PLAYER_VIDEO_LOOP_INDEX_OVERFLOW",
                        )
                    })?;
                    restarts.push((
                        index,
                        video.request.clone(),
                        video.session,
                        loop_index,
                        video.started_at_ms,
                        (
                            video.duration_us,
                            video.expected_frame_count,
                            video.expected_decoded_byte_count,
                        ),
                    ));
                    break;
                }
                if video.request.fence.is_some() {
                    source
                        .complete_video_fence(&video.request)
                        .map_err(|error| media_error("player.video.complete", error))?;
                }
                completed.push(index);
                break;
            }
        }
        for (index, request, old_session, loop_index, started_at_ms, expected) in restarts {
            let mut replacement = self
                .open_video_stream(source, executor, request, started_at_ms)
                .await?;
            if (
                replacement.duration_us,
                replacement.expected_frame_count,
                replacement.expected_decoded_byte_count,
            ) != expected
            {
                self.pending_video_closes.push(old_session);
                self.pending_video_closes.push(replacement.session);
                return Err(media_error(
                    "player.video.stream.loop",
                    "ASTRA_PLAYER_VIDEO_LOOP_STREAM_IDENTITY_MISMATCH",
                ));
            }
            replacement.loop_index = loop_index;
            self.pending_video_closes.push(old_session);
            self.active_videos[index] = replacement;
        }
        for index in completed.into_iter().rev() {
            let video = self.active_videos.remove(index);
            self.pending_video_closes.push(video.session);
        }
        self.close_pending_video_streams(source, executor).await?;
        Ok(())
    }
}

fn decoded_bgra_frame(
    width: u32,
    height: u32,
    mut bytes: astra_byte_source::OwnedByteBuffer,
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
    if bytes.len() != expected || expected == 0 {
        return Err(media_error(
            "player.video.contract",
            "ASTRA_PLAYER_VIDEO_BUFFER_SIZE",
        ));
    }
    let rgba8 = bytes.make_mut_vec();
    for pixel in rgba8.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    astra_media_core::TextureFrame::from_buffer(
        width,
        height,
        astra_media_core::OwnedPixelBuffer::from_owned(bytes),
    )
    .map_err(|error| {
        media_error(
            "player.video.contract",
            format!("ASTRA_PLAYER_VIDEO_TEXTURE: {error}"),
        )
    })
}

fn media_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(id: &str, samples: usize) -> PcmAsset {
        PcmAsset::from_canonical_samples(id, vec![0.0; samples]).unwrap()
    }

    #[astra_headless_test::test]
    fn decoded_audio_recency_evicts_the_oldest_live_generation() {
        let mut host = NativeVnProductMediaHost::new(8);
        host.max_decoded_cache_bytes = 32;
        let first = AudioCacheKey::new("first", "wav", 4);
        let second = AudioCacheKey::new("second", "wav", 4);
        let third = AudioCacheKey::new("third", "wav", 4);

        host.cache_audio(first.clone(), pcm("first", 4));
        host.cache_audio(second.clone(), pcm("second", 4));
        assert!(host.cached_audio(&first).is_some());
        host.cache_audio(third.clone(), pcm("third", 4));

        assert!(host.decoded_audio_cache.contains_key(&first));
        assert!(!host.decoded_audio_cache.contains_key(&second));
        assert!(host.decoded_audio_cache.contains_key(&third));
        assert_eq!(host.decoded_audio_cache_bytes, 32);
    }

    #[astra_headless_test::test]
    fn decoded_audio_recency_queue_compacts_stale_generations() {
        let mut host = NativeVnProductMediaHost::new(8);
        let key = AudioCacheKey::new("stable", "wav", 4);
        host.cache_audio(key.clone(), pcm("stable", 4));
        for _ in 0..128 {
            assert!(host.touch_cached_audio(&key));
        }
        host.compact_audio_recency_if_needed();

        assert!(host.decoded_audio_lru.len() <= 33);
        let live_epoch = host.decoded_audio_cache[&key].last_access_epoch;
        assert_eq!(host.decoded_audio_lru.back(), Some(&(key, live_epoch)));
    }
}
