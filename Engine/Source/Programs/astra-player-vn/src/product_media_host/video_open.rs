use super::*;

impl NativeVnProductMediaHost {
    pub(super) async fn open_video_stream(
        &mut self,
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
        // This owner survives dropping the future while a decode reply is pending.
        self.pending_video_closes.push(plan.session);
        let opened = async {
            if request.is_cancelled() {
                return Err(media_error(
                    "player.video.decode.cancelled",
                    "ASTRA_PLAYER_VIDEO_REQUEST_CANCELLED",
                ));
            }
            let decoded = executor
                .execute_decode_submit(plan.session, plan.decode)
                .await
                .map_err(|error| media_error("player.video.decode.start", error))?;
            if request.is_cancelled() {
                return Err(media_error(
                    "player.video.decode.cancelled",
                    "ASTRA_PLAYER_VIDEO_REQUEST_CANCELLED",
                ));
            }
            let astra_platform::DecodeOutput::VideoStreamStart {
                duration_us: Some(duration_us),
                frame_count,
                decoded_byte_count,
            } = decoded.output
            else {
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
        .await;
        match opened {
            Ok(video) => {
                self.pending_video_closes.retain(|id| *id != plan.session);
                Ok(video)
            }
            Err(error) => {
                if let Err(cleanup) = self.close_pending_video_streams(source, executor).await {
                    return Err(media_error(
                        "player.video.decode.cleanup",
                        format!("{error}; close failed: {cleanup}"),
                    ));
                }
                Err(error)
            }
        }
    }

    pub(super) async fn close_pending_video_streams(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        while let Some(session) = self.pending_video_closes.first().copied() {
            let close = source
                .prepare_video_stream_close(session)
                .map_err(|error| media_error("player.video.close.prepare", error))?;
            executor
                .execute_decode_close(session, close)
                .await
                .map_err(|error| media_error("player.video.close", error))?;
            self.pending_video_closes.remove(0);
        }
        Ok(())
    }
}
