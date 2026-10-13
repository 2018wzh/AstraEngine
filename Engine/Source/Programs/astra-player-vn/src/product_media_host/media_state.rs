use super::*;

impl NativeVnProductMediaHost {
    pub(super) const MAX_COMPLETION_CHAIN: usize = 1_024;

    pub(super) const MAX_DECODED_AUDIO_SAMPLES: usize = 10_000_000;

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
            clock: clock::PlaybackClock::default(),
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

    pub fn last_audio_meter(&self) -> Option<crate::NativeVnAudioMeterSnapshot> {
        self.audio.last_meter()
    }

    pub fn has_active_voice(&self) -> bool {
        self.audio.has_active_voice()
    }

    pub fn submitted_audio_timeline(&self) -> Result<Vec<f32>, PlatformError> {
        self.audio.submitted_timeline()
    }

    pub async fn set_output_paused(
        &mut self,
        paused: bool,
        executor: &PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        self.audio.set_output_paused(paused, executor).await
    }

    pub fn snapshot(&self) -> NativeVnProductMediaSnapshot {
        NativeVnProductMediaSnapshot {
            schema: "astra.player.native_vn_media_snapshot.v4".into(),
            playback_time_ms: self.clock.time_ms(),
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
                    frame_end_us: video.presented_end_us,
                    loop_duration_us: video.loop_duration_us,
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
                .chain(self.restored_videos.iter().cloned())
                .collect(),
        }
    }

    pub(crate) fn validate_snapshot_data(
        snapshot: &NativeVnProductMediaSnapshot,
    ) -> Result<(), PlatformError> {
        if snapshot.schema != "astra.player.native_vn_media_snapshot.v4" {
            return Err(media_error(
                "player.media.restore",
                "ASTRA_PLAYER_MEDIA_SNAPSHOT_INVALID",
            ));
        }
        if snapshot
            .timeline
            .last_time_ms
            .is_some_and(|time| time > snapshot.playback_time_ms)
            || snapshot.active_videos.iter().any(|video| {
                video.started_at_ms > snapshot.playback_time_ms
                    || (video.next_frame == 0) != (video.frame_end_us == 0)
                    || video.loop_duration_us == Some(0)
                    || (video.loop_index > 0 && video.loop_duration_us.is_none())
                    || video
                        .loop_duration_us
                        .is_some_and(|duration| video.frame_end_us > duration)
            })
        {
            return Err(media_error(
                "player.media.restore",
                "ASTRA_PLAYER_MEDIA_CLOCK_SNAPSHOT_INVALID",
            ));
        }
        PlayerTimelineScheduler::restore(snapshot.timeline.clone())
            .map_err(|error| media_error("player.media.timeline.restore", error))?;
        NativeVnProductAudioHost::validate_snapshot_data(&snapshot.audio)
    }

    pub(crate) fn validate_restore(
        &self,
        snapshot: &NativeVnProductMediaSnapshot,
    ) -> Result<(), PlatformError> {
        Self::validate_snapshot_data(snapshot)?;
        self.audio.validate_restore(&snapshot.audio)
    }

    pub fn restore(&mut self, snapshot: NativeVnProductMediaSnapshot) -> Result<(), PlatformError> {
        self.validate_restore(&snapshot)?;
        let timeline = PlayerTimelineScheduler::restore(snapshot.timeline)
            .map_err(|error| media_error("player.media.timeline.restore", error))?;
        self.audio.restore(snapshot.audio)?;
        self.timeline = timeline;
        self.clock = clock::PlaybackClock::restored(snapshot.playback_time_ms);
        self.completed_signals = snapshot.completed_signals.into_iter().collect();
        self.pending_video_closes
            .extend(self.active_videos.drain(..).map(|video| {
                video.request.scope.cancel();
                video.session
            }));
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
        let now_ms = self.clock.advance(now_ms)?;
        let completed = self
            .timeline
            .poll(now_ms)
            .map_err(|error| media_error("player.timeline.poll", error))?;
        self.process_at_playback_time(source, executor, now_ms, completed, render_audio_tick)
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
        completed: Vec<PlayerTimelineCompletion>,
        render_audio_tick: bool,
    ) -> Result<(), PlatformError> {
        let now_ms = self.clock.advance(now_ms)?;
        self.process_at_playback_time(source, executor, now_ms, completed, render_audio_tick)
            .await
    }

    pub(super) async fn process_at_playback_time(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
        mut completed: Vec<PlayerTimelineCompletion>,
        render_audio_tick: bool,
    ) -> Result<(), PlatformError> {
        self.active_videos.retain(|video| {
            if video.request.is_cancelled() {
                self.pending_video_closes.push(video.session);
                false
            } else {
                true
            }
        });
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

    pub(super) fn add_profile_duration(
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
        executor.sink_mut().cleanup_pending_decode_opens().await?;
        self.pending_video_closes
            .extend(self.active_videos.iter().map(|video| video.session));
        self.active_videos.clear();
        self.restored_videos.clear();
        self.close_pending_video_streams(source, executor).await?;
        self.audio.shutdown(source, executor).await
    }
}
