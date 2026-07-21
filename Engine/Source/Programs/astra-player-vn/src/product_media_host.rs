use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};

use astra_core::Hash256;
use astra_media::{PcmAsset, CANONICAL_CHANNELS, CANONICAL_SAMPLE_RATE};
use astra_platform::{PlatformError, PlatformErrorCode};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerTimelineCompletion,
    PlayerTimelineScheduler, PlayerTimelineSchedulerSnapshot,
};

use crate::{
    NativeVnAudioOutput, NativeVnAudioPreloadRequest, NativeVnDecodedCacheBudget,
    NativeVnHostCommandSource, NativeVnProductAudioHost, NativeVnVideoRequest,
    DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
};

pub struct NativeVnProductMediaHost {
    audio: NativeVnProductAudioHost,
    timeline: PlayerTimelineScheduler,
    completed_signals: BTreeSet<String>,
    active_videos: Vec<ActiveVideoStream>,
    restored_videos: Vec<NativeVnVideoStreamSnapshot>,
    decoded_audio_cache: BTreeMap<Hash256, CachedDecodedAudio>,
    decoded_audio_lru: VecDeque<Hash256>,
    decoded_audio_cache_bytes: u64,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
    max_decoded_cache_bytes: u64,
    performance: Option<NativeVnMediaPerformanceSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeVnMediaPerformanceSample {
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
    decoded_hash: String,
    byte_size: u64,
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
            restored_videos: Vec::new(),
            decoded_audio_cache: BTreeMap::new(),
            decoded_audio_lru: VecDeque::new(),
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

    pub fn has_active_video(&self) -> bool {
        !self.active_videos.is_empty()
    }

    pub fn decoded_cache_bytes(&self) -> u64 {
        self.decoded_audio_cache_bytes
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
        self.prewarm_pending_audio(source, executor).await?;
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
                let (audio, decoded_hash, cache_hit) =
                    if let Some(cached) = self.cached_audio(&request.encoded_hash) {
                        (
                            cached.asset.with_identity(request.asset_id.clone()),
                            cached.decoded_hash,
                            true,
                        )
                    } else {
                        let cached = self
                            .decode_audio(
                                source,
                                executor,
                                &NativeVnAudioPreloadRequest {
                                    asset_id: request.asset_id.clone(),
                                    codec: request.codec.clone(),
                                    encoded_bytes: request.encoded_bytes.clone(),
                                    encoded_hash: request.encoded_hash,
                                },
                            )
                            .await?;
                        (
                            cached.asset.with_identity(request.asset_id.clone()),
                            cached.decoded_hash,
                            false,
                        )
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
                tracing::info!(
                    event = "astra.player.audio.started",
                    command_id = %request.command_id,
                    command = %request.command,
                    asset_id = %request.asset_id,
                    encoded_hash = %request.encoded_hash,
                    decoded_hash = %decoded_hash,
                    cache_hit,
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

    fn cached_audio(&mut self, encoded_hash: &Hash256) -> Option<CachedDecodedAudio> {
        let cached = self.decoded_audio_cache.get(encoded_hash)?.clone();
        self.decoded_audio_lru.retain(|hash| hash != encoded_hash);
        self.decoded_audio_lru.push_back(*encoded_hash);
        Some(cached)
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
        let mut required_hashes = BTreeSet::new();
        for request in &requests {
            required_hashes.insert(request.encoded_hash);
            if self.cached_audio(&request.encoded_hash).is_none() {
                self.decode_audio(source, executor, request).await?;
            }
        }
        if required_hashes
            .iter()
            .any(|hash| !self.decoded_audio_cache.contains_key(hash))
        {
            return Err(media_error(
                "player.audio.prewarm",
                "ASTRA_PLAYER_AUDIO_PREWARM_BUDGET_EXCEEDED",
            ));
        }
        tracing::info!(
            event = "astra.player.audio.prewarm.completed",
            asset_count = required_hashes.len(),
            decoded_cache_bytes = self.decoded_audio_cache_bytes,
            cache_budget_bytes = self.max_decoded_cache_bytes,
            "prewarmed reusable entry-story audio into the bounded decoded cache"
        );
        Ok(())
    }

    async fn decode_audio(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: &NativeVnAudioPreloadRequest,
    ) -> Result<CachedDecodedAudio, PlatformError> {
        let decode_started = self.performance.as_ref().map(|_| Instant::now());
        let decode = source
            .prepare_audio_preload_decode(request)
            .map_err(|error| media_error("player.audio.decode.prepare", error))?;
        let decoded = executor
            .execute_decode_lifecycle(decode)
            .await
            .map_err(|error| media_error("player.audio.decode", error))?;
        self.add_profile_duration(decode_started, |sample, duration| {
            sample.provider_decode_ns = sample.provider_decode_ns.saturating_add(duration);
        })?;
        let convert_started = self.performance.as_ref().map(|_| Instant::now());
        let audio = PlayerDecodedAudio::parse(
            &decoded.format,
            &decoded.bytes,
            Self::MAX_DECODED_AUDIO_SAMPLES,
        )
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
        let cached = CachedDecodedAudio {
            byte_size: (asset.samples.len() as u64)
                .saturating_mul(std::mem::size_of::<f32>() as u64),
            asset,
            decoded_hash: decoded.hash,
        };
        self.cache_audio(
            request.encoded_hash,
            cached.asset.clone(),
            cached.decoded_hash.clone(),
        );
        Ok(cached)
    }

    fn cache_audio(&mut self, encoded_hash: Hash256, asset: PcmAsset, decoded_hash: String) {
        let byte_size =
            (asset.samples.len() as u64).saturating_mul(std::mem::size_of::<f32>() as u64);
        if byte_size > self.max_decoded_cache_bytes {
            tracing::debug!(
                event = "astra.player.audio.cache.bypass",
                encoded_hash = %encoded_hash,
                byte_size,
                cache_budget_bytes = self.max_decoded_cache_bytes,
                "decoded audio exceeded the bounded session cache budget"
            );
            return;
        }
        if let Some(previous) = self.decoded_audio_cache.remove(&encoded_hash) {
            self.decoded_audio_cache_bytes = self
                .decoded_audio_cache_bytes
                .saturating_sub(previous.byte_size);
            self.decoded_audio_lru.retain(|hash| hash != &encoded_hash);
        }
        while self.decoded_audio_cache_bytes.saturating_add(byte_size)
            > self.max_decoded_cache_bytes
        {
            let Some(evicted_hash) = self.decoded_audio_lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.decoded_audio_cache.remove(&evicted_hash) {
                self.decoded_audio_cache_bytes = self
                    .decoded_audio_cache_bytes
                    .saturating_sub(evicted.byte_size);
                tracing::debug!(
                    event = "astra.player.audio.cache.evicted",
                    encoded_hash = %evicted_hash,
                    byte_size = evicted.byte_size,
                    "evicted least-recently-used decoded audio from the session cache"
                );
            }
        }
        self.decoded_audio_cache.insert(
            encoded_hash,
            CachedDecodedAudio {
                asset,
                decoded_hash,
                byte_size,
            },
        );
        self.decoded_audio_lru.push_back(encoded_hash);
        self.decoded_audio_cache_bytes = self.decoded_audio_cache_bytes.saturating_add(byte_size);
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
        rgba8: rgba8.into(),
    })
}

fn media_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
