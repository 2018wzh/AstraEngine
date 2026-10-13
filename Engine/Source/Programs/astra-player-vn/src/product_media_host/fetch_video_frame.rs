use super::*;

impl NativeVnProductMediaHost {
    pub(super) async fn fetch_video_frame(
        max_decode_output_bytes: u64,
        max_video_frames: u64,
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
                let end_us = frame.pts_us.checked_add(frame.duration_us).ok_or_else(|| {
                    media_error(
                        "player.video.stream.frame",
                        "ASTRA_PLAYER_VIDEO_FRAME_TIME_OVERFLOW",
                    )
                })?;
                if frame.sequence != expected_sequence
                    || frame.sequence > max_video_frames
                    || frame.bgra8.len() as u64 > max_decode_output_bytes
                    || video
                        .loop_duration_us
                        .is_some_and(|duration| end_us > duration)
                {
                    return Err(media_error(
                        "player.video.stream.frame",
                        "ASTRA_PLAYER_VIDEO_FRAME_IDENTITY_MISMATCH",
                    ));
                }
                video.decoded_end_us = end_us;
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
                if frame_count == 0
                    || video.decoded_end_us == 0
                    || video
                        .loop_duration_us
                        .is_some_and(|duration| duration != video.decoded_end_us)
                    || video.next_frame != frame_count
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
                video.loop_duration_us = Some(video.decoded_end_us);
                video.reached_end = true;
                Ok(None)
            }
            _ => Err(media_error(
                "player.video.stream.next",
                "ASTRA_PLAYER_VIDEO_STREAM_OUTPUT_INVALID",
            )),
        }
    }

    pub(super) async fn restore_video_streams(
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
            video.loop_duration_us = snapshot.loop_duration_us;
            let restored_frame = async {
                let mut restored_frame = None;
                for expected in 1..=snapshot.next_frame {
                    let frame = Self::fetch_video_frame(
                        self.max_decode_output_bytes,
                        self.max_video_frames,
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
                        return Err(media_error(
                            "player.video.restore.cursor",
                            "ASTRA_PLAYER_VIDEO_RESTORE_SEQUENCE_MISMATCH",
                        ));
                    }
                    video.next_frame = expected;
                    video.presented_end_us = video.decoded_end_us;
                    restored_frame = Some(frame);
                }
                if video.decoded_byte_count != snapshot.decoded_byte_count
                    || video.presented_end_us != snapshot.frame_end_us
                {
                    return Err(media_error(
                        "player.video.restore.identity",
                        "ASTRA_PLAYER_VIDEO_STREAM_IDENTITY_MISMATCH",
                    ));
                }
                Ok::<_, PlatformError>(restored_frame)
            }
            .await;
            let restored_frame = match restored_frame {
                Ok(frame) => frame,
                Err(error) => {
                    self.pending_video_closes.push(video.session);
                    return Err(error);
                }
            };
            video.loop_index = snapshot.loop_index;
            if let Some(frame) = restored_frame {
                let result = async {
                    let texture = decoded_bgra_frame(frame.width, frame.height, frame.bgra8)?;
                    let present = source
                        .bind_decoded_video_frame(&video.request, texture, false)
                        .map_err(|error| media_error("player.video.restore.bind", error))?;
                    executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| media_error("player.video.restore.present", error))?;
                    Ok::<_, PlatformError>(())
                }
                .await;
                if let Err(error) = result {
                    self.pending_video_closes.push(video.session);
                    return Err(error);
                }
            }
            self.active_videos.push(video);
        }
        Ok(())
    }

    pub(super) fn cached_audio(&mut self, key: &AudioCacheKey) -> Option<CachedDecodedAudio> {
        if !self.touch_cached_audio(key) {
            return None;
        }
        self.decoded_audio_cache.get(key).cloned()
    }

    pub(super) fn touch_cached_audio(&mut self, key: &AudioCacheKey) -> bool {
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

    pub(super) async fn prewarm_pending_audio(
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

    pub(super) async fn decode_audio(
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

    pub(super) async fn decode_audio_uncached(
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

    pub(super) fn cache_audio_without_eviction(
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

    pub(super) fn cache_audio(&mut self, key: AudioCacheKey, asset: PcmAsset) {
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
}
