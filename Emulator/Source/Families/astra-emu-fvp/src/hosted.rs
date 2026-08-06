//! Astra-side, product-neutral conversion of RFVP hosted output.
//!
//! This module intentionally contains no RuntimeWorld or platform handle.  It
//! is the only place where RFVP's typed hosted delta is translated into the
//! existing renderer-neutral family packet.

use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioSampleFormat, LegacyBlendMode,
    LegacyDrawV1, LegacyPayload, LegacySceneResourceOperationV7, LegacySceneResourceStateV1,
    LegacySceneTransactionV7, LegacyScissorV1, LegacyTextureFormat, LegacyVertexV1,
    LegacyVideoCommandV1, LegacyVideoMode,
};
use rfvp_hosted::{
    host_api::{BlendMode, DrawSolidCommand, PixelFormat, TextureId},
    hosted::{HostedAudioOperation, HostedSceneOperation, HostedStepDelta, HostedVideoOperation},
};

const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

/// Session-owned translator for RFVP's semantic scene operations.  Its
/// metadata is intentionally serializable, so a save/restore boundary can
/// reconstruct the same incremental resource validation without retaining
/// decoded pixels outside the renderer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostedSceneTranslator {
    resources: LegacySceneResourceStateV1,
    // RFVP can allocate or update a texture before beginning the frame that
    // first samples it. Keep the bounded semantic operation until that frame
    // closes so the renderer receives one atomic commit rather than losing a
    // resource-only delta between steps.
    pending_live_resources: Vec<LegacySceneResourceOperationV7>,
    pending_live_upload_bytes: usize,
    rehydrate_resources: bool,
    next_generation: u64,
}

impl HostedSceneTranslator {
    pub fn snapshot(&self) -> LegacySceneResourceStateV1 {
        self.resources.clone()
    }

    pub fn restore(&mut self, _resources: LegacySceneResourceStateV1) {
        // RFVP restore replays live texture creation through the hosted
        // renderer port. The host-side resource cache is not persisted with
        // the provider snapshot, so the next complete packet must establish
        // a new resource epoch rather than collide with pre-restore metadata.
        self.resources = LegacySceneResourceStateV1::default();
        self.pending_live_resources.clear();
        self.pending_live_upload_bytes = 0;
        self.rehydrate_resources = true;
        self.next_generation = 1;
    }

    /// Converts one RFVP frame by moving pixel allocations directly into the
    /// typed Family ABI transaction. No serialized scene mirror is created.
    pub fn translate(
        &mut self,
        delta: &mut HostedStepDelta,
    ) -> Result<Option<LegacySceneTransactionV7>, HostedAdapterError> {
        let mut frame: Option<(u32, u32)> = None;
        let mut ended = false;
        let mut presented = false;
        let mut resources = std::mem::take(&mut self.pending_live_resources);
        let mut draws = Vec::new();
        let mut bytes = self.pending_live_upload_bytes;
        if self.next_generation == 0 {
            self.next_generation = 1;
        }

        for operation in std::mem::take(&mut delta.scene) {
            match operation {
                HostedSceneOperation::CreateTexture(texture) => {
                    let pixels = texture
                        .pixels
                        .ok_or(HostedAdapterError::TextureWithoutPixels(texture.id))?;
                    let (format, pixels) = texture_payload_owned(
                        &mut bytes,
                        texture.id,
                        texture.desc.width,
                        texture.desc.height,
                        texture.desc.format,
                        pixels,
                    )?;
                    let generation = take_generation(&mut self.next_generation)?;
                    resources.push(LegacySceneResourceOperationV7::CreateTexture {
                        texture_id: texture.id.0,
                        generation,
                        width: texture.desc.width,
                        height: texture.desc.height,
                        format,
                        pixels: LegacyPayload::Native(pixels),
                    });
                }
                HostedSceneOperation::UpdateTexture(update) => {
                    let (format, pixels) = texture_payload_owned(
                        &mut bytes,
                        update.id,
                        update.rect.width,
                        update.rect.height,
                        update.format,
                        update.pixels,
                    )?;
                    let generation = take_generation(&mut self.next_generation)?;
                    resources.push(LegacySceneResourceOperationV7::UpdateTexture {
                        texture_id: update.id.0,
                        generation,
                        x: update.rect.x,
                        y: update.rect.y,
                        width: update.rect.width,
                        height: update.rect.height,
                        format,
                        pixels: LegacyPayload::Native(pixels),
                    });
                }
                HostedSceneOperation::DestroyTexture(id) => {
                    resources.push(LegacySceneResourceOperationV7::DestroyTexture {
                        texture_id: id.0,
                        generation: take_generation(&mut self.next_generation)?,
                    });
                }
                HostedSceneOperation::BeginFrame { width, height, .. } => {
                    if frame.replace((width, height)).is_some() || ended || presented {
                        return Err(HostedAdapterError::FrameBoundary);
                    }
                }
                HostedSceneOperation::DrawSprite(draw) => {
                    if frame.is_none() || ended || presented {
                        return Err(HostedAdapterError::FrameBoundary);
                    }
                    draws.push(sprite_draw(&draw));
                }
                HostedSceneOperation::DrawSolid(command) => {
                    if frame.is_none() || ended || presented {
                        return Err(HostedAdapterError::FrameBoundary);
                    }
                    draws.push(solid_draw(&command));
                }
                HostedSceneOperation::EndFrame => {
                    if frame.is_none() || ended || presented {
                        return Err(HostedAdapterError::FrameBoundary);
                    }
                    ended = true;
                }
                HostedSceneOperation::Present => {
                    if !ended || presented {
                        return Err(HostedAdapterError::FrameBoundary);
                    }
                    presented = true;
                }
            }
        }

        match (frame, ended, presented) {
            (None, false, false) => {
                self.pending_live_resources = resources;
                self.pending_live_upload_bytes = bytes;
                Ok(None)
            }
            (Some((width, height)), true, true) => {
                let mut transaction = LegacySceneTransactionV7 {
                    sequence: 0,
                    width,
                    height,
                    resources,
                    draws,
                    reset_resources: self.rehydrate_resources,
                };
                let next = self
                    .resources
                    .validate_live(&transaction)
                    .map_err(|error| {
                        tracing::error!(
                            event = "astra.emu.fvp.hosted_live_scene_invalid",
                            diagnostic_code = error.code(),
                            retained_texture_count = self.resources.textures.len(),
                            staged_resource_count = transaction.resources.len(),
                            draw_count = transaction.draws.len(),
                            "RFVP hosted live scene transaction failed validation"
                        );
                        HostedAdapterError::InvalidPacket(error.to_string())
                    })?;
                self.resources = next;
                self.pending_live_upload_bytes = 0;
                self.rehydrate_resources = false;
                transaction.sequence = self.next_generation;
                Ok(Some(transaction))
            }
            _ => Err(HostedAdapterError::FrameBoundary),
        }
    }
}

fn take_generation(next: &mut u64) -> Result<u64, HostedAdapterError> {
    let generation = (*next).max(1);
    *next = generation
        .checked_add(1)
        .ok_or(HostedAdapterError::GenerationExhausted)?;
    Ok(generation)
}

/// Converts video deltas into host-resolved resource commands. Encoded bytes
/// remain behind the active VFS policy and are not copied into an ABI packet.
pub fn video_commands_from_delta(
    frame_index: u64,
    operations: Vec<HostedVideoOperation>,
) -> Result<Vec<LegacyVideoCommandV1>, HostedAdapterError> {
    operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| match operation {
            HostedVideoOperation::Play {
                resource_uri,
                byte_len,
                modal_with_audio,
                stage_width,
                stage_height,
            } => {
                if byte_len == 0 || byte_len > 512 * 1024 * 1024 {
                    return Err(HostedAdapterError::VideoResourceBounds);
                }
                let command = LegacyVideoCommandV1::Play {
                    playback_id: format!("rfvp-{frame_index}-{index}"),
                    resource_uri,
                    mode: if modal_with_audio {
                        LegacyVideoMode::ModalWithAudio
                    } else {
                        LegacyVideoMode::LayerNoAudio
                    },
                    stage_width,
                    stage_height,
                };
                command
                    .validate()
                    .map_err(|error| HostedAdapterError::InvalidPacket(error.code().to_owned()))?;
                Ok(command)
            }
        })
        .collect()
}

/// Converts the hosted core's single audio transaction into validated host
/// commands.  This keeps PCM/encoded buffers bounded by RFVP and avoids the
/// former second audio DTO and cross-layer command mutex.
pub fn audio_commands_from_delta(
    operations: Vec<HostedAudioOperation>,
) -> Result<Vec<LegacyAudioCommandV1>, HostedAdapterError> {
    operations
        .into_iter()
        .map(|operation| {
            let command = match operation {
                HostedAudioOperation::LoadResource {
                    id,
                    kind,
                    resource_uri,
                } => LegacyAudioCommandV1::LoadResource {
                    stream_id: id.0,
                    encoding: match kind {
                        rfvp_hosted::host_api::EncodedAudioKind::Unknown => {
                            LegacyAudioEncoding::Unknown
                        }
                        rfvp_hosted::host_api::EncodedAudioKind::Wav => LegacyAudioEncoding::Wav,
                        rfvp_hosted::host_api::EncodedAudioKind::Ogg => LegacyAudioEncoding::Ogg,
                        rfvp_hosted::host_api::EncodedAudioKind::Mp3 => LegacyAudioEncoding::Mp3,
                        rfvp_hosted::host_api::EncodedAudioKind::Flac => LegacyAudioEncoding::Flac,
                    },
                    resource_uri,
                },
                HostedAudioOperation::LoadEncoded { .. } => {
                    return Err(HostedAdapterError::EncodedAudioRequiresResource);
                }
                HostedAudioOperation::CreateStream { id, desc } => {
                    LegacyAudioCommandV1::CreateStream {
                        stream_id: id.0,
                        sample_rate: desc.sample_rate,
                        channels: desc.channels,
                        sample_format: match desc.sample_format {
                            rfvp_hosted::host_api::AudioSampleFormat::I16 => {
                                LegacyAudioSampleFormat::I16
                            }
                            rfvp_hosted::host_api::AudioSampleFormat::F32 => {
                                LegacyAudioSampleFormat::F32
                            }
                        },
                    }
                }
                HostedAudioOperation::SubmitI16 { id, samples } => {
                    LegacyAudioCommandV1::SubmitI16 {
                        stream_id: id.0,
                        samples,
                    }
                }
                HostedAudioOperation::SubmitF32 { id, samples } => {
                    LegacyAudioCommandV1::SubmitF32 {
                        stream_id: id.0,
                        samples,
                    }
                }
                HostedAudioOperation::Play {
                    id,
                    params,
                    fade_in_ms,
                } => LegacyAudioCommandV1::Play {
                    stream_id: id.0,
                    volume: params.volume,
                    pan: params.pan,
                    repeat: params.repeat,
                    fade_in_ms,
                },
                HostedAudioOperation::Stop { id, fade_ms } => LegacyAudioCommandV1::Stop {
                    stream_id: id.0,
                    fade_ms,
                },
                HostedAudioOperation::Pause(id) => LegacyAudioCommandV1::Pause { stream_id: id.0 },
                HostedAudioOperation::Resume(id) => {
                    LegacyAudioCommandV1::Resume { stream_id: id.0 }
                }
                HostedAudioOperation::SetParams { id, params } => LegacyAudioCommandV1::SetParams {
                    stream_id: id.0,
                    volume: params.volume,
                    pan: params.pan,
                    repeat: params.repeat,
                },
                HostedAudioOperation::SetMasterVolume(volume) => {
                    LegacyAudioCommandV1::MasterVolume { volume }
                }
                HostedAudioOperation::DestroyStream(id) => {
                    LegacyAudioCommandV1::DestroyStream { stream_id: id.0 }
                }
                HostedAudioOperation::Tick { .. } => return Ok(None),
            };
            command
                .validate()
                .map_err(|error| HostedAdapterError::InvalidPacket(error.code().to_owned()))?;
            Ok(Some(command))
        })
        .filter_map(Result::transpose)
        .collect()
}

fn texture_payload_owned(
    bytes: &mut usize,
    id: TextureId,
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
) -> Result<(LegacyTextureFormat, Vec<u8>), HostedAdapterError> {
    let format = match format {
        PixelFormat::Rgba8 => LegacyTextureFormat::Rgba8,
        PixelFormat::LumaA8 => LegacyTextureFormat::LumaAlpha8,
        _ => return Err(HostedAdapterError::TextureFormat(id)),
    };
    let channels = match format {
        LegacyTextureFormat::Rgba8 => 4usize,
        LegacyTextureFormat::LumaAlpha8 => 2usize,
    };
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or(HostedAdapterError::TextureBounds(id))?;
    if pixels.len() != expected {
        return Err(HostedAdapterError::TextureBounds(id));
    }
    *bytes = bytes
        .checked_add(expected)
        .ok_or(HostedAdapterError::UploadBudget)?;
    if *bytes > MAX_UPLOAD_BYTES {
        return Err(HostedAdapterError::UploadBudget);
    }
    Ok((format, pixels))
}

fn map_blend(blend: BlendMode) -> LegacyBlendMode {
    match blend {
        BlendMode::Opaque => LegacyBlendMode::Opaque,
        BlendMode::Alpha => LegacyBlendMode::Alpha,
        BlendMode::Add => LegacyBlendMode::Add,
        BlendMode::Multiply => LegacyBlendMode::Multiply,
        BlendMode::Screen => LegacyBlendMode::Screen,
    }
}

fn solid_draw(command: &DrawSolidCommand) -> LegacyDrawV1 {
    let x0 = command.rect.x as f32;
    let y0 = command.rect.y as f32;
    let x1 = command.rect.x.saturating_add(command.rect.width) as f32;
    let y1 = command.rect.y.saturating_add(command.rect.height) as f32;
    let color = [
        command.color.r,
        command.color.g,
        command.color.b,
        command.color.a,
    ];
    LegacyDrawV1 {
        texture_id: u32::MAX,
        vertices: [
            LegacyVertexV1 {
                position: [x0, y1],
                tex_coord: [0.0, 1.0],
                color,
            },
            LegacyVertexV1 {
                position: [x0, y0],
                tex_coord: [0.0, 0.0],
                color,
            },
            LegacyVertexV1 {
                position: [x1, y1],
                tex_coord: [1.0, 1.0],
                color,
            },
            LegacyVertexV1 {
                position: [x1, y0],
                tex_coord: [1.0, 0.0],
                color,
            },
        ],
        blend: map_blend(command.blend),
        scissor: command.scissor.map(|scissor| LegacyScissorV1 {
            x: scissor.x,
            y: scissor.y,
            width: scissor.width,
            height: scissor.height,
        }),
    }
}

fn sprite_draw(draw: &rfvp_hosted::host_api::DrawSpriteCommand) -> LegacyDrawV1 {
    LegacyDrawV1 {
        texture_id: draw.texture.0,
        vertices: draw.vertices.map(|vertex| LegacyVertexV1 {
            position: vertex.position,
            tex_coord: vertex.tex_coord,
            color: [
                vertex.color.r,
                vertex.color.g,
                vertex.color.b,
                vertex.color.a,
            ],
        }),
        blend: map_blend(draw.blend),
        scissor: draw.scissor.map(|scissor| LegacyScissorV1 {
            x: scissor.x,
            y: scissor.y,
            width: scissor.width,
            height: scissor.height,
        }),
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostedAdapterError {
    #[error("ASTRA_FVP_HOSTED_FRAME_BOUNDARY")]
    FrameBoundary,
    #[error("ASTRA_FVP_HOSTED_TEXTURE_NO_PIXELS:{0:?}")]
    TextureWithoutPixels(TextureId),
    #[error("ASTRA_FVP_HOSTED_TEXTURE_FORMAT:{0:?}")]
    TextureFormat(TextureId),
    #[error("ASTRA_FVP_HOSTED_TEXTURE_BOUNDS:{0:?}")]
    TextureBounds(TextureId),
    #[error("ASTRA_FVP_HOSTED_UPLOAD_BUDGET")]
    UploadBudget,
    #[error("ASTRA_FVP_HOSTED_GENERATION_EXHAUSTED")]
    GenerationExhausted,
    #[error("ASTRA_FVP_HOSTED_PACKET:{0}")]
    InvalidPacket(String),
    #[error("ASTRA_FVP_HOSTED_VIDEO_RESOURCE_BOUNDS")]
    VideoResourceBounds,
    #[error("ASTRA_FVP_HOSTED_AUDIO_RESOURCE_REQUIRED")]
    EncodedAudioRequiresResource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfvp_hosted::{
        host_api::{TextureDesc, TextureRect},
        hosted::{HostedAudioOperation, HostedTextureData, HostedTickResult},
    };

    fn delta(scene: Vec<HostedSceneOperation>) -> HostedStepDelta {
        HostedStepDelta {
            tick: HostedTickResult {
                frame_index: 7,
                consumed_events: 0,
                elapsed_us: 16_667,
            },
            scene,
            audio: Vec::<HostedAudioOperation>::new(),
            video: Vec::new(),
            text: Vec::new(),
            logs: Vec::new(),
            log_dropped_count: 0,
            copy_telemetry: Default::default(),
        }
    }

    #[test]
    fn typed_scene_retains_texture_metadata_for_partial_uploads() {
        let mut translator = HostedSceneTranslator::default();
        let mut create = delta(vec![
            HostedSceneOperation::CreateTexture(HostedTextureData {
                id: TextureId(9),
                desc: TextureDesc {
                    width: 2,
                    height: 1,
                    format: PixelFormat::Rgba8,
                    mip_count: 1,
                },
                pixels: Some(vec![0, 0, 0, 255, 255, 255, 255, 255]),
            }),
            HostedSceneOperation::BeginFrame {
                width: 640,
                height: 480,
                clear: None,
            },
            HostedSceneOperation::EndFrame,
            HostedSceneOperation::Present,
        ]);
        translator
            .translate(&mut create)
            .expect("create transaction translates");

        let mut update = delta(vec![
            HostedSceneOperation::UpdateTexture(rfvp_hosted::hosted::HostedTextureUpdate {
                id: TextureId(9),
                rect: TextureRect {
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                format: PixelFormat::Rgba8,
                pixels: vec![1, 2, 3, 4],
            }),
            HostedSceneOperation::BeginFrame {
                width: 640,
                height: 480,
                clear: None,
            },
            HostedSceneOperation::EndFrame,
            HostedSceneOperation::Present,
        ]);
        let transaction = translator
            .translate(&mut update)
            .expect("partial transaction translates")
            .expect("complete frame");
        assert!(matches!(
            transaction.resources.as_slice(),
            [LegacySceneResourceOperationV7::UpdateTexture { x: 1, .. }]
        ));
        assert!(translator.snapshot().textures.contains_key(&9));
    }

    #[test]
    fn retains_a_resource_only_delta_until_a_later_frame_commits_it() {
        let mut translator = HostedSceneTranslator::default();
        let mut resource_only = delta(vec![HostedSceneOperation::CreateTexture(
            HostedTextureData {
                id: TextureId(9),
                desc: TextureDesc {
                    width: 1,
                    height: 1,
                    format: PixelFormat::Rgba8,
                    mip_count: 1,
                },
                pixels: Some(vec![0, 0, 0, 255]),
            },
        )]);
        assert!(translator
            .translate(&mut resource_only)
            .expect("resource-only delta is retained")
            .is_none());

        let mut frame = delta(vec![
            HostedSceneOperation::BeginFrame {
                width: 640,
                height: 480,
                clear: None,
            },
            HostedSceneOperation::EndFrame,
            HostedSceneOperation::Present,
        ]);
        let transaction = translator
            .translate(&mut frame)
            .expect("later frame commits retained resource")
            .expect("complete frame");
        assert!(matches!(
            transaction.resources.as_slice(),
            [LegacySceneResourceOperationV7::CreateTexture { texture_id: 9, .. }]
        ));
        assert!(translator.snapshot().textures.contains_key(&9));
    }

    #[test]
    fn restored_translator_starts_a_new_resource_epoch() {
        let mut translator = HostedSceneTranslator::default();
        translator.restore(LegacySceneResourceStateV1::default());
        let mut rehydration = delta(vec![
            HostedSceneOperation::CreateTexture(HostedTextureData {
                id: TextureId(9),
                desc: TextureDesc {
                    width: 1,
                    height: 1,
                    format: PixelFormat::Rgba8,
                    mip_count: 1,
                },
                pixels: Some(vec![0, 0, 0, 255]),
            }),
            HostedSceneOperation::BeginFrame {
                width: 640,
                height: 480,
                clear: None,
            },
            HostedSceneOperation::EndFrame,
            HostedSceneOperation::Present,
        ]);
        let transaction = translator
            .translate(&mut rehydration)
            .expect("rehydration transaction translates")
            .expect("complete frame");
        assert!(transaction.reset_resources);
    }

    #[test]
    fn converts_video_as_a_vfs_resource_command() {
        let mut input = delta(Vec::new());
        input.video.push(HostedVideoOperation::Play {
            resource_uri: "movie/opening.wmv".into(),
            byte_len: 4_096,
            modal_with_audio: true,
            stage_width: 640,
            stage_height: 480,
        });
        let commands = video_commands_from_delta(input.tick.frame_index, input.video)
            .expect("valid video resource converts");
        assert!(matches!(
            commands.as_slice(),
            [LegacyVideoCommandV1::Play { resource_uri, .. }] if resource_uri == "movie/opening.wmv"
        ));
    }

    #[test]
    fn converts_pcm_audio_and_blocks_encoded_bytes_without_a_resource_response() {
        let create = HostedAudioOperation::CreateStream {
            id: rfvp_hosted::host_api::AudioStreamId(4),
            desc: rfvp_hosted::host_api::AudioStreamDesc {
                sample_rate: 48_000,
                channels: 2,
                sample_format: rfvp_hosted::host_api::AudioSampleFormat::I16,
            },
        };
        assert!(matches!(
            audio_commands_from_delta(vec![create.clone()])
                .expect("PCM stream converts")
                .as_slice(),
            [LegacyAudioCommandV1::CreateStream { stream_id: 4, .. }]
        ));
        let encoded = HostedAudioOperation::LoadEncoded {
            id: rfvp_hosted::host_api::AudioStreamId(4),
            kind: rfvp_hosted::host_api::EncodedAudioKind::Ogg,
            bytes: vec![0],
        };
        assert_eq!(
            audio_commands_from_delta(vec![create.clone(), encoded]),
            Err(HostedAdapterError::EncodedAudioRequiresResource)
        );
        let resource = HostedAudioOperation::LoadResource {
            id: rfvp_hosted::host_api::AudioStreamId(4),
            kind: rfvp_hosted::host_api::EncodedAudioKind::Ogg,
            resource_uri: "audio/theme.ogg".into(),
        };
        assert!(matches!(
            audio_commands_from_delta(vec![create, resource]).expect("resource audio converts").as_slice(),
            [LegacyAudioCommandV1::CreateStream { .. }, LegacyAudioCommandV1::LoadResource { resource_uri, .. }]
                if resource_uri == "audio/theme.ogg"
        ));
    }
}
