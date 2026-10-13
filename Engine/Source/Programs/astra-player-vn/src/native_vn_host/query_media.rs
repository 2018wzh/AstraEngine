use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn prepare_audio_decode(
        &mut self,
        request: &NativeVnAudioRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        self.prepare_audio_asset_decode(
            &request.asset_id,
            &request.codec,
            request.encoded_bytes.clone(),
            request.encoded_length,
        )
    }

    pub fn prepare_audio_preload_decode(
        &mut self,
        request: &NativeVnAudioPreloadRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        self.prepare_audio_asset_decode(
            &request.asset_id,
            &request.codec,
            request.encoded_bytes.clone(),
            request.encoded_length,
        )
    }

    pub(super) fn prepare_audio_asset_decode(
        &mut self,
        asset_id: &str,
        codec: &str,
        encoded_bytes: astra_byte_source::OwnedByteBuffer,
        encoded_length: u64,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        if encoded_bytes.is_empty() || encoded_bytes.len() as u64 != encoded_length {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_ENCODED_LENGTH: {}",
                asset_id
            )));
        }
        let session = self.next_media_resource()?;
        Ok(PlayerDecodeLifecyclePlan {
            session,
            open: PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenDecode {
                sequence: self.next_command_sequence()?,
                session,
                kind: PlayerDecodeKind::Audio,
            }])?,
            decode: PlayerHostCommandBatch::new(vec![PlayerHostCommand::Decode {
                sequence: self.next_command_sequence()?,
                request_sequence: 1,
                session,
                kind: PlayerDecodeKind::Audio,
                codec: codec.to_string(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: astra_player_core::PlayerDecodeStreamAction::OneShot,
                bytes: encoded_bytes,
            }])?,
            close: PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
                sequence: self.next_command_sequence()?,
                session,
            }])?,
        })
    }

    pub fn prepare_video_decode(
        &mut self,
        request: &NativeVnVideoRequest,
    ) -> Result<PlayerDecodeLifecyclePlan, NativeVnHostError> {
        self.validate_video_request(request)?;
        if request.encoded_bytes.is_empty()
            || request.encoded_bytes.len() as u64 != request.encoded_length
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_VIDEO_ENCODED_LENGTH: {}",
                request.asset_id
            )));
        }
        let session = self.next_media_resource()?;
        Ok(PlayerDecodeLifecyclePlan {
            session,
            open: PlayerHostCommandBatch::new(vec![PlayerHostCommand::OpenDecode {
                sequence: self.next_command_sequence()?,
                session,
                kind: PlayerDecodeKind::Video,
            }])?,
            decode: PlayerHostCommandBatch::new(vec![PlayerHostCommand::Decode {
                sequence: self.next_command_sequence()?,
                request_sequence: 1,
                session,
                kind: PlayerDecodeKind::Video,
                codec: request.codec.clone(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: astra_player_core::PlayerDecodeStreamAction::Start,
                bytes: request.encoded_bytes.clone(),
            }])?,
            close: PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
                sequence: self.next_command_sequence()?,
                session,
            }])?,
        })
    }

    pub(crate) fn prepare_video_stream_next(
        &mut self,
        session: astra_player_core::PlayerHostResourceId,
        request_sequence: u64,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        PlayerHostCommandBatch::new(vec![PlayerHostCommand::Decode {
            sequence: self.next_command_sequence()?,
            request_sequence,
            session,
            kind: PlayerDecodeKind::Video,
            codec: String::new(),
            description: Vec::new(),
            sample_rate: None,
            channels: None,
            coded_width: None,
            coded_height: None,
            keyframe: false,
            stream_action: astra_player_core::PlayerDecodeStreamAction::Next,
            bytes: Vec::new().into(),
        }])
        .map_err(Into::into)
    }

    pub(crate) fn prepare_video_stream_close(
        &mut self,
        session: astra_player_core::PlayerHostResourceId,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        PlayerHostCommandBatch::new(vec![PlayerHostCommand::CloseDecode {
            sequence: self.next_command_sequence()?,
            session,
        }])
        .map_err(Into::into)
    }

    pub fn bind_decoded_video_frame(
        &mut self,
        request: &NativeVnVideoRequest,
        frame: TextureFrame,
        complete: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.validate_video_request(request)?;
        let result = self.bind_decoded_video_frame_inner(request, frame, complete);
        if result.is_err() {
            self.presentation_failed = true;
            self.media_scope.cancel();
        }
        result
    }

    pub(super) fn bind_decoded_video_frame_inner(
        &mut self,
        request: &NativeVnVideoRequest,
        frame: TextureFrame,
        complete: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.stage_director
            .start_video(&request.layer)
            .map_err(stage_director_error)?;
        self.store_texture(
            request.asset_id.clone(),
            frame,
            &BTreeSet::from([request.asset_id.clone()]),
        )?;
        if complete {
            let completed = self
                .stage_director
                .complete_video(&request.layer)
                .map_err(stage_director_error)?;
            self.pending_stage_completions.extend(completed);
        }
        self.render_with_stage_refresh(&[], 0, true)
    }

    pub(crate) fn complete_video_fence(
        &mut self,
        request: &NativeVnVideoRequest,
    ) -> Result<(), NativeVnHostError> {
        self.validate_video_request(request)?;
        let completed = self
            .stage_director
            .complete_video(&request.layer)
            .map_err(stage_director_error)?;
        self.pending_stage_completions.extend(completed);
        Ok(())
    }

    pub(crate) fn rehydrate_video_request(
        &mut self,
        snapshot: &crate::NativeVnVideoStreamSnapshot,
    ) -> Result<NativeVnVideoRequest, NativeVnHostError> {
        let asset = self.asset_store.load_media(&snapshot.asset_id)?;
        if asset.byte_length != snapshot.encoded_length {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_VIDEO_RESTORE_LENGTH_MISMATCH: {}",
                snapshot.asset_id
            )));
        }
        Ok(NativeVnVideoRequest {
            scope: self.replace_video_scope(&snapshot.layer),
            layer: snapshot.layer.clone(),
            asset_id: snapshot.asset_id.clone(),
            codec: asset.codec.clone(),
            encoded_bytes: asset.bytes.clone(),
            encoded_length: asset.byte_length,
            alpha_millionths: snapshot.alpha_millionths,
            looping: snapshot.looping,
            fence: snapshot.fence.clone(),
        })
    }

    pub fn complete_wait(
        &mut self,
        fence: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.command(VnPlayerCommand::CompleteWait {
            fence: fence.into(),
        })
    }
}
