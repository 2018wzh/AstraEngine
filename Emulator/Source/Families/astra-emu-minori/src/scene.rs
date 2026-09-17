use crate::{MinoriMountedVfs, MinoriRuntimeState, MinoriTextRenderer};
use astra_byte_source::OwnedByteBuffer;
use astra_emu_family_api::{FamilyError, FamilyResult};
use astra_emu_sdk::TextureCache;
use astra_media_core::{BlendMode, RectI, SceneCommand, TextureFrame};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;
use std::{num::NonZeroUsize, sync::Arc};

const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;

mod stage;
mod stand;

#[cfg(test)]
mod tests;

pub(crate) fn read_asset(
    archive: &MinoriMountedVfs,
    uri: &str,
    limit: u64,
) -> FamilyResult<OwnedByteBuffer> {
    let stat = archive.stat(uri).map_err(core_error)?;
    if stat.size == 0 || stat.size > limit.min(MAX_ASSET_BYTES) {
        return Err(error(
            "ASTRA_EMU_MINORI_ASSET_BOUND",
            "asset size is outside the supported bound",
        ));
    }
    archive
        .read_range(uri, 0, stat.size)
        .map(|r| r.bytes)
        .map_err(core_error)
}

pub(crate) fn core_error(e: crate::CoreError) -> FamilyError {
    FamilyError::invalid(e.code(), e.message())
}
pub(crate) fn error(code: &str, message: &str) -> FamilyError {
    FamilyError::invalid(code, message)
}

pub(crate) struct Scene {
    archive: Arc<MinoriMountedVfs>,
    renderer: WgpuOffscreenRenderer,
    sequence: u64,
    text: MinoriTextRenderer,
    textures: TextureCache,
    stand_offsets: lru::LruCache<String, stand::Offsets>,
    width: u32,
    height: u32,
    pub pixels: Arc<[u8]>,
}

impl Scene {
    pub fn new(archive: Arc<MinoriMountedVfs>, width: u32, height: u32) -> FamilyResult<Self> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new())
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_RENDERER",
                    "hardware GPU renderer could not be created",
                )
            })?
            .with_default_compositing(astra_media_core::SceneCompositing2D::EncodedSrgb);
        tracing::info!(event = "astra.emu.minori.gpu.created", backend = %renderer.identity().backend,
            device_type = %renderer.identity().device_type);
        let text = MinoriTextRenderer::new().map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_FONT",
                "font renderer could not be created",
            )
        })?;
        Ok(Self {
            archive,
            renderer,
            text,
            sequence: 0,
            textures: TextureCache::new(NonZeroUsize::new(32).unwrap(), MAX_IMAGE_BYTES, 8192)
                .map_err(|_| {
                    error(
                        "ASTRA_EMU_MINORI_IMAGE_BOUND",
                        "invalid texture cache budget",
                    )
                })?,
            width,
            height,
            stand_offsets: lru::LruCache::new(NonZeroUsize::new(32).unwrap()),
            pixels: vec![0; width as usize * height as usize * 4].into(),
        })
    }
    fn texture(&mut self, uri: &str) -> FamilyResult<TextureFrame> {
        if let Some(frame) = self.textures.get(uri) {
            return Ok(frame.clone());
        }
        let bytes = read_asset(&self.archive, uri, MAX_ASSET_BYTES)?;
        let frame = self.textures.decode(uri.into(), &bytes).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_IMAGE_DECODE",
                "image could not be decoded within its bounds",
            )
        })?;
        Ok(frame)
    }
    fn layer(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        uri: &str,
        x: i32,
        y: i32,
        opacity: f32,
    ) -> FamilyResult<()> {
        self.layer_with_blend(commands, uri, [x, y], opacity, BlendMode::Alpha)
    }
    fn layer_with_blend(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        uri: &str,
        [x, y]: [i32; 2],
        opacity: f32,
        blend: BlendMode,
    ) -> FamilyResult<()> {
        let frame = self.texture(uri)?;
        commands.push(SceneCommand::Texture {
            id: format!("layer:{}", commands.len()),
            destination: RectI {
                x,
                y,
                width: frame.width,
                height: frame.height,
            },
            frame,
            opacity,
            blend,
        });
        Ok(())
    }
    pub fn render(
        &mut self,
        state: &MinoriRuntimeState,
        message: Option<&(String, Option<String>)>,
        choices: Option<(&[String], u32)>,
    ) -> FamilyResult<()> {
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        if let Some(stage) = &state.stage {
            self.stage(&mut commands, stage)?;
        }
        if let Some(effect) = &state.effect {
            let current = effect
                .resources
                .get(effect.visible_current_index as usize)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MINORI_EFFECT_STATE",
                        "effect resource index is invalid",
                    )
                })?;
            let next = effect
                .resources
                .get(effect.visible_next_index as usize)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MINORI_EFFECT_STATE",
                        "effect resource index is invalid",
                    )
                })?;
            let alpha = effect.visible_alpha_255 as f32 / 255.0;
            if let Some(uri) = current {
                self.layer(
                    &mut commands,
                    uri,
                    0,
                    0,
                    if next.is_some() { 1.0 } else { 1.0 - alpha },
                )?;
            }
            if let Some(uri) = next {
                self.layer(&mut commands, uri, 0, 0, alpha)?;
            }
        }
        if let Some(panel) = &state.panel {
            if panel.mode != 1 {
                return Err(error(
                    "ASTRA_EMU_MINORI_PANEL_MODE",
                    "panel mode is not verified",
                ));
            }
            let frame = self.texture(&panel.resource_uri)?;
            self.layer(
                &mut commands,
                &panel.resource_uri,
                0,
                self.height as i32 - frame.height as i32 + 64,
                1.0,
            )?;
        }
        commands.extend(
            self.text
                .commands(
                    message.map(|(text, speaker)| (text.as_str(), speaker.as_deref())),
                    choices,
                )
                .map_err(|code| error(&code, "message could not be rendered"))?,
        );
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MINORI_FRAME_SEQUENCE", "frame sequence overflow"))?;
        self.pixels = self
            .renderer
            .render(&SceneFrame {
                sequence,
                width: self.width,
                height: self.height,
                clear_rgba: [0, 0, 0, 255],
                commands,
                semantics: None,
            })
            .map_err(|cause| {
                tracing::error!(event = "astra.emu.minori.gpu.failed", operation = %cause.operation,
                    code = ?cause.code, "GPU scene composition failed");
                error("ASTRA_EMU_MINORI_RENDER", "GPU scene composition failed")
            })?
            .rgba8;
        self.sequence = sequence;
        Ok(())
    }
}
