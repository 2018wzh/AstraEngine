use crate::{MusicaMountedVfs, MusicaRuntimeState, MusicaTextRenderer};
use astra_byte_source::OwnedByteBuffer;
use astra_emu_family_api::{FamilyError, FamilyResult};
use astra_emu_sdk::TextureCache;
use astra_media_core::{BlendMode, RectI, SceneCommand, TextureFrame};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;
use std::{num::NonZeroUsize, sync::Arc};

const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;

mod character;
mod particles;
mod stage;
mod stand;
mod texture;
mod wscroll2;

#[cfg(test)]
mod tests;

pub(crate) fn read_asset(
    archive: &MusicaMountedVfs,
    uri: &str,
    limit: u64,
) -> FamilyResult<OwnedByteBuffer> {
    let stat = archive.stat(uri).map_err(core_error)?;
    if stat.size == 0 || stat.size > limit.min(MAX_ASSET_BYTES) {
        return Err(error(
            "ASTRA_EMU_MUSICA_ASSET_BOUND",
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
    archive: Arc<MusicaMountedVfs>,
    renderer: WgpuOffscreenRenderer,
    sequence: u64,
    text: MusicaTextRenderer,
    textures: TextureCache,
    wscroll2_sync: lru::LruCache<String, Vec<i32>>,
    stand_offsets: lru::LruCache<String, stand::Offsets>,
    width: u32,
    height: u32,
    pub pixels: Arc<[u8]>,
}

impl Scene {
    pub fn new(archive: Arc<MusicaMountedVfs>, width: u32, height: u32) -> FamilyResult<Self> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new())
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RENDERER",
                    "hardware GPU renderer could not be created",
                )
            })?
            .with_default_compositing(astra_media_core::SceneCompositing2D::EncodedSrgb);
        tracing::info!(
            event = "astra.emu.musica.gpu.created",
            backend = renderer.identity().backend.as_str(),
            device_type = renderer.identity().device_type.as_str()
        );
        let text = MusicaTextRenderer::new().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_FONT",
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
                        "ASTRA_EMU_MUSICA_IMAGE_BOUND",
                        "invalid texture cache budget",
                    )
                })?,
            width,
            height,
            wscroll2_sync: lru::LruCache::new(NonZeroUsize::new(16).unwrap()),
            stand_offsets: lru::LruCache::new(NonZeroUsize::new(32).unwrap()),
            pixels: vec![0; width as usize * height as usize * 4].into(),
        })
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
        state: &MusicaRuntimeState,
        message: Option<&(String, Option<String>)>,
        choices: Option<(&[String], u32)>,
    ) -> FamilyResult<()> {
        let history = if state.system_ui.page == crate::MusicaSystemPage::Backlog {
            let index = state.system_ui.backlog_cursor.ok_or_else(|| {
                error("ASTRA_EMU_MUSICA_RUNTIME_BACKLOG", "missing backlog cursor")
            })? as usize;
            let entry = state.backlog.get(index).ok_or_else(|| {
                error("ASTRA_EMU_MUSICA_RUNTIME_BACKLOG", "invalid backlog cursor")
            })?;
            Some((
                entry.text.clone(),
                Some(format!(
                    "Backlog {}/{} | {}",
                    index + 1,
                    state.backlog.len(),
                    entry.speaker.as_deref().unwrap_or("")
                )),
            ))
        } else {
            None
        };
        let mode_message =
            if state.system_ui.play_mode != crate::MusicaPlayMode::Normal && history.is_none() {
                message.map(|(text, speaker)| {
                    (
                        text.clone(),
                        Some(format!(
                            "{} | {}",
                            if state.system_ui.play_mode == crate::MusicaPlayMode::Auto {
                                "AUTO"
                            } else {
                                "SKIP"
                            },
                            speaker.as_deref().unwrap_or("")
                        )),
                    )
                })
            } else {
                None
            };
        let message = history.as_ref().or(mode_message.as_ref()).or(message);
        let choices = if history.is_some() { None } else { choices };
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        if let Some(shake) = &state.screen_shake {
            crate::runtime::shake::validate_screen_shake_state(shake)
                .map_err(|cause| error(cause.diagnostic_code(), "invalid screen shake state"))?;
            commands.push(SceneCommand::PushTransform {
                transform: astra_media_core::Transform2D::translation(
                    shake.offset[0] as f32,
                    shake.offset[1] as f32,
                ),
            });
            commands.push(SceneCommand::PushClip {
                rect: RectI {
                    x: 0,
                    y: 0,
                    width: self.width,
                    height: self.height,
                },
            });
        }
        if let Some(scroll) = &state.scroll_xf {
            crate::runtime::scroll_xf::validate_scroll_xf_state(scroll)
                .map_err(|cause| error(cause.diagnostic_code(), "invalid scrollxf state"))?;
            commands.push(SceneCommand::PushClip {
                rect: RectI {
                    x: 0,
                    y: 0,
                    width: (scroll.visible_extent[0] as u32).min(self.width),
                    height: (scroll.visible_extent[1] as u32).min(self.height),
                },
            });
            commands.push(SceneCommand::PushTransform {
                transform: astra_media_core::Transform2D::translation(
                    -scroll.visible_offset[0] as f32,
                    -scroll.visible_offset[1] as f32,
                ),
            });
        }
        if state.wscroll2.is_some() {
            self.wscroll2_stage(&mut commands, state)?;
        } else if let Some(stage) = &state.stage {
            self.stage(&mut commands, stage)?;
        }
        self.characters(&mut commands, state)?;
        if let Some(effect) = &state.effect {
            let current = effect
                .resources
                .get(effect.visible_current_index as usize)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_EFFECT_STATE",
                        "effect resource index is invalid",
                    )
                })?;
            let next = effect
                .resources
                .get(effect.visible_next_index as usize)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_EFFECT_STATE",
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
            let y = match panel.mode {
                1 => {
                    let frame = self.texture(&panel.resource_uri)?;
                    i32::try_from(i64::from(self.height) - i64::from(frame.height) + 64).map_err(
                        |_| error("ASTRA_EMU_MUSICA_PANEL_POSITION", "panel position overflow"),
                    )?
                }
                3 => 0,
                _ => return Err(error("ASTRA_EMU_MUSICA_PANEL_MODE", "invalid panel mode")),
            };
            self.layer(&mut commands, &panel.resource_uri, 0, y, 1.0)?;
        }
        if state.scroll_xf.is_some() {
            commands.push(SceneCommand::PopTransform);
            commands.push(SceneCommand::PopClip);
        }
        self.particles(&mut commands, state)?;
        if state.screen_shake.is_some() {
            commands.push(SceneCommand::PopClip);
            commands.push(SceneCommand::PopTransform);
        }
        commands.extend(
            self.text
                .commands(
                    message.map(|(text, speaker)| (text.as_str(), speaker.as_deref())),
                    choices,
                )
                .map_err(|code| error(&code, "message could not be rendered"))?,
        );
        self.submit(commands)
    }
    pub fn render_movie(&mut self, frame: TextureFrame) -> FamilyResult<()> {
        self.submit(vec![SceneCommand::Texture {
            id: "musica.movie".into(),
            destination: RectI {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            frame,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }])
    }
    fn submit(&mut self, commands: Vec<SceneCommand>) -> FamilyResult<()> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_FRAME_SEQUENCE", "frame sequence overflow"))?;
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
                tracing::error!(
                    event = "astra.emu.musica.gpu.failed",
                    operation = %cause.operation,
                    code = ?cause.code,
                    "GPU scene composition failed"
                );
                error("ASTRA_EMU_MUSICA_RENDER", "GPU scene composition failed")
            })?
            .rgba8;
        self.sequence = sequence;
        Ok(())
    }
}
