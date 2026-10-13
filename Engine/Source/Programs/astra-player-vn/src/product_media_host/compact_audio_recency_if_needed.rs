use super::*;

impl NativeVnProductMediaHost {
    pub(super) fn compact_audio_recency_if_needed(&mut self) {
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

    pub(super) async fn present_due_video_frames(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        now_ms: u64,
    ) -> Result<(), PlatformError> {
        let mut completed = Vec::new();
        let mut restarts = Vec::new();
        for (index, video) in self.active_videos.iter_mut().enumerate() {
            if video.request.is_cancelled() {
                completed.push(index);
                continue;
            }
            let elapsed_us = now_ms
                .checked_sub(video.started_at_ms)
                .and_then(|elapsed| elapsed.checked_mul(1_000))
                .ok_or_else(|| {
                    media_error(
                        "player.video.stream.clock",
                        "ASTRA_PLAYER_VIDEO_TIME_OVERFLOW",
                    )
                })?;
            loop {
                if video.pending_frame.is_none() && !video.reached_end {
                    video.pending_frame = Self::fetch_video_frame(
                        self.max_decode_output_bytes,
                        self.max_video_frames,
                        source,
                        executor,
                        video,
                    )
                    .await?;
                }
                if let Some(frame) = video.pending_frame.as_ref() {
                    let loop_offset = video
                        .loop_index
                        .checked_mul(video.loop_duration_us.unwrap_or(0))
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
                    video.presented_end_us = video.decoded_end_us;
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
                    .and_then(|loop_index| {
                        loop_index
                            .checked_mul(video.loop_duration_us.expect("EOS finalizes duration"))
                    })
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
                            video.loop_duration_us,
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
                replacement.expected_frame_count,
                replacement.expected_decoded_byte_count,
            ) != (expected.1, expected.2)
            {
                self.pending_video_closes.push(old_session);
                self.pending_video_closes.push(replacement.session);
                return Err(media_error(
                    "player.video.stream.loop",
                    "ASTRA_PLAYER_VIDEO_LOOP_STREAM_IDENTITY_MISMATCH",
                ));
            }
            replacement.loop_duration_us = expected.0;
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

pub(super) fn decoded_bgra_frame(
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
    for pixel in rgba8.as_chunks_mut::<4>().0.iter_mut() {
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

pub(super) fn media_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
