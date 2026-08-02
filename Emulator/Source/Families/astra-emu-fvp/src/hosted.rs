//! Astra-side, product-neutral conversion of RFVP hosted output.
//!
//! This module intentionally contains no RuntimeWorld or platform handle.  It
//! is the only place where RFVP's typed hosted delta is translated into the
//! existing renderer-neutral family packet.

use astra_core::Hash256;
use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyRenderFrameV1, LegacyScissorV1,
    LegacyTextureFormat, LegacyTextureUpdateV1, LegacyVertexV1, LegacyVideoCommandV1,
    LegacyVideoMode,
};
use rfvp_hosted::{
    host_api::{BlendMode, DrawSolidCommand, PixelFormat, TextureId},
    hosted::{HostedSceneOperation, HostedStepDelta, HostedVideoOperation},
};

const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

/// Converts exactly one RFVP hosted transaction into one renderer packet.
/// Frame-boundary violations and unsupported semantic operations are blocking;
/// callers never receive a partially converted presentation packet.
pub fn scene_packet_from_delta(
    delta: &HostedStepDelta,
) -> Result<Option<LegacyRenderFrameV1>, HostedAdapterError> {
    let mut frame: Option<(u32, u32)> = None;
    let mut ended = false;
    let mut presented = false;
    let mut updates = Vec::new();
    let mut draws = Vec::new();
    let mut bytes = 0usize;

    for operation in &delta.scene {
        match operation {
            HostedSceneOperation::CreateTexture(texture) => {
                let Some(pixels) = &texture.pixels else {
                    return Err(HostedAdapterError::TextureWithoutPixels(texture.id));
                };
                push_texture(
                    &mut updates,
                    &mut bytes,
                    texture.id,
                    texture.desc.width,
                    texture.desc.height,
                    texture.desc.format,
                    pixels,
                )?;
            }
            HostedSceneOperation::UpdateTexture(update) => {
                if update.rect.x != 0 || update.rect.y != 0 {
                    return Err(HostedAdapterError::PartialTextureUpdate(update.id));
                }
                push_texture(
                    &mut updates,
                    &mut bytes,
                    update.id,
                    update.rect.width,
                    update.rect.height,
                    update.format,
                    &update.pixels,
                )?;
            }
            HostedSceneOperation::DestroyTexture(id) => {
                return Err(HostedAdapterError::TextureDestroyRequiresScenePacket(*id));
            }
            HostedSceneOperation::BeginFrame { width, height, .. } => {
                if frame.replace((*width, *height)).is_some() || ended || presented {
                    return Err(HostedAdapterError::FrameBoundary);
                }
            }
            HostedSceneOperation::DrawSprite(draw) => {
                if frame.is_none() || ended || presented {
                    return Err(HostedAdapterError::FrameBoundary);
                }
                draws.push(LegacyDrawV1 {
                    texture_id: draw.texture.0,
                    vertices: draw.vertices.map(|vertex| LegacyVertexV1 {
                        position: vertex.position,
                        tex_coord: vertex.tex_coord,
                        color: [vertex.color.r, vertex.color.g, vertex.color.b, vertex.color.a],
                    }),
                    blend: map_blend(draw.blend),
                    scissor: draw.scissor.map(|scissor| LegacyScissorV1 {
                        x: scissor.x,
                        y: scissor.y,
                        width: scissor.width,
                        height: scissor.height,
                    }),
                });
            }
            HostedSceneOperation::DrawSolid(command) => {
                if frame.is_none() || ended || presented {
                    return Err(HostedAdapterError::FrameBoundary);
                }
                draws.push(solid_draw(command));
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
        (None, false, false) => Ok(None),
        (Some((width, height)), true, true) => {
            let packet = LegacyRenderFrameV1 {
                width,
                height,
                texture_updates: updates,
                draws,
            };
            packet
                .validate()
                .map_err(|error| HostedAdapterError::InvalidPacket(error.code().to_owned()))?;
            Ok(Some(packet))
        }
        _ => Err(HostedAdapterError::FrameBoundary),
    }
}

/// Converts video deltas into host-resolved resource commands. Encoded bytes
/// remain behind the active VFS policy and are not copied into an ABI packet.
pub fn video_commands_from_delta(
    delta: &HostedStepDelta,
) -> Result<Vec<LegacyVideoCommandV1>, HostedAdapterError> {
    delta
        .video
        .iter()
        .enumerate()
        .map(|(index, operation)| match operation {
            HostedVideoOperation::Play {
                resource_uri,
                byte_len,
                modal_with_audio,
                stage_width,
                stage_height,
            } => {
                if *byte_len == 0 || *byte_len > 512 * 1024 * 1024 {
                    return Err(HostedAdapterError::VideoResourceBounds);
                }
                let command = LegacyVideoCommandV1::Play {
                    playback_id: format!("rfvp-{}-{index}", delta.tick.frame_index),
                    resource_uri: resource_uri.clone(),
                    mode: if *modal_with_audio {
                        LegacyVideoMode::ModalWithAudio
                    } else {
                        LegacyVideoMode::LayerNoAudio
                    },
                    stage_width: *stage_width,
                    stage_height: *stage_height,
                };
                command
                    .validate()
                    .map_err(|error| HostedAdapterError::InvalidPacket(error.code().to_owned()))?;
                Ok(command)
            }
        })
        .collect()
}

fn push_texture(
    updates: &mut Vec<LegacyTextureUpdateV1>,
    bytes: &mut usize,
    id: TextureId,
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: &[u8],
) -> Result<(), HostedAdapterError> {
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
    *bytes = bytes.checked_add(expected).ok_or(HostedAdapterError::UploadBudget)?;
    if *bytes > MAX_UPLOAD_BYTES {
        return Err(HostedAdapterError::UploadBudget);
    }
    updates.push(LegacyTextureUpdateV1 {
        texture_id: id.0,
        width,
        height,
        format,
        content_hash: Hash256::from_sha256(pixels),
        pixels: pixels.to_vec(),
    });
    Ok(())
}

fn map_blend(blend: BlendMode) -> LegacyBlendMode {
    match blend {
        BlendMode::Opaque | BlendMode::Alpha => LegacyBlendMode::Alpha,
        BlendMode::Add => LegacyBlendMode::Add,
        BlendMode::Multiply => LegacyBlendMode::Multiply,
        BlendMode::Screen => LegacyBlendMode::Alpha,
    }
}

fn solid_draw(command: &DrawSolidCommand) -> LegacyDrawV1 {
    let x0 = command.rect.x as f32;
    let y0 = command.rect.y as f32;
    let x1 = command.rect.x.saturating_add(command.rect.width) as f32;
    let y1 = command.rect.y.saturating_add(command.rect.height) as f32;
    let color = [command.color.r, command.color.g, command.color.b, command.color.a];
    LegacyDrawV1 {
        texture_id: u32::MAX,
        vertices: [
            LegacyVertexV1 { position: [x0, y1], tex_coord: [0.0, 1.0], color },
            LegacyVertexV1 { position: [x0, y0], tex_coord: [0.0, 0.0], color },
            LegacyVertexV1 { position: [x1, y1], tex_coord: [1.0, 1.0], color },
            LegacyVertexV1 { position: [x1, y0], tex_coord: [1.0, 0.0], color },
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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostedAdapterError {
    #[error("ASTRA_FVP_HOSTED_FRAME_BOUNDARY")]
    FrameBoundary,
    #[error("ASTRA_FVP_HOSTED_TEXTURE_NO_PIXELS:{0:?}")]
    TextureWithoutPixels(TextureId),
    #[error("ASTRA_FVP_HOSTED_PARTIAL_TEXTURE:{0:?}")]
    PartialTextureUpdate(TextureId),
    #[error("ASTRA_FVP_HOSTED_TEXTURE_DESTROY:{0:?}")]
    TextureDestroyRequiresScenePacket(TextureId),
    #[error("ASTRA_FVP_HOSTED_TEXTURE_FORMAT:{0:?}")]
    TextureFormat(TextureId),
    #[error("ASTRA_FVP_HOSTED_TEXTURE_BOUNDS:{0:?}")]
    TextureBounds(TextureId),
    #[error("ASTRA_FVP_HOSTED_UPLOAD_BUDGET")]
    UploadBudget,
    #[error("ASTRA_FVP_HOSTED_PACKET:{0}")]
    InvalidPacket(String),
    #[error("ASTRA_FVP_HOSTED_VIDEO_RESOURCE_BOUNDS")]
    VideoResourceBounds,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfvp_hosted::{
        host_api::{ColorRgba, RectI32, TextureRect},
        hosted::{HostedAudioOperation, HostedTickResult},
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
        }
    }

    #[test]
    fn converts_one_complete_semantic_frame() {
        let frame = scene_packet_from_delta(&delta(vec![
            HostedSceneOperation::BeginFrame { width: 640, height: 480, clear: None },
            HostedSceneOperation::DrawSolid(DrawSolidCommand {
                rect: RectI32 { x: 4, y: 8, width: 16, height: 32 },
                color: ColorRgba::BLACK,
                blend: BlendMode::Alpha,
                scissor: None,
            }),
            HostedSceneOperation::EndFrame,
            HostedSceneOperation::Present,
        ]))
        .expect("complete hosted frame converts")
        .expect("complete hosted frame is present");

        assert_eq!((frame.width, frame.height), (640, 480));
        assert_eq!(frame.draws.len(), 1);
        assert!(frame.texture_updates.is_empty());
    }

    #[test]
    fn rejects_partial_texture_updates_without_a_partial_commit() {
        let error = scene_packet_from_delta(&delta(vec![HostedSceneOperation::UpdateTexture(
            rfvp_hosted::hosted::HostedTextureUpdate {
                id: TextureId(9),
                rect: TextureRect { x: 1, y: 0, width: 1, height: 1 },
                format: PixelFormat::Rgba8,
                pixels: vec![0, 0, 0, 255],
            },
        )]))
        .expect_err("v1 packet cannot represent a partial texture update");

        assert_eq!(error, HostedAdapterError::PartialTextureUpdate(TextureId(9)));
    }
}
