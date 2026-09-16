use crate::{CmvsArchive, CmvsPs2aVmState};
use astra_emu_sdk::{CoreError, TextureCache};
use astra_media_core::{BlendMode, RectI, SceneCommand, TextureFrame};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;
use std::{collections::BTreeMap, num::NonZeroUsize};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CmvsTextureSlot {
    Parent(u8),
    Child { parent: u8, child: u16 },
}

/// Core-owned GPU composition of the recovered parent/child texture stage.
pub struct CmvsScene {
    renderer: WgpuOffscreenRenderer,
    textures: TextureCache,
    bindings: BTreeMap<CmvsTextureSlot, String>,
    sequence: u64,
}

impl CmvsScene {
    pub fn new() -> Result<Self, CoreError> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new())
            .map_err(|_| error("ASTRA_EMU_CMVS_GPU_CREATE"))?;
        tracing::info!(event = "cmvs.gpu.created", backend = %renderer.identity().backend,
            device_type = %renderer.identity().device_type);
        Ok(Self {
            renderer,
            textures: TextureCache::new(NonZeroUsize::new(512).unwrap(), 1024 * 1024 * 1024, 16384)
                .map_err(|_| error("ASTRA_EMU_CMVS_TEXTURE_BUDGET"))?,
            bindings: BTreeMap::new(),
            sequence: 0,
        })
    }

    pub fn bind(
        &mut self,
        slot: CmvsTextureSlot,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        if self.bindings.len() >= 512 && !self.bindings.contains_key(&slot) {
            return Err(error("ASTRA_EMU_CMVS_TEXTURE_BINDINGS"));
        }
        // Explicit loads decode again even when the URI has previously been cached.
        let image = archive.decode_pb_image(uri)?;
        let frame = TextureFrame {
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into(),
        };
        self.textures
            .insert(uri.to_owned(), frame)
            .map_err(|_| error("ASTRA_EMU_CMVS_TEXTURE_BUDGET"))?;
        self.bindings.insert(slot, uri.to_owned());
        Ok(())
    }

    pub fn render(
        &mut self,
        vm: &CmvsPs2aVmState,
        requested: CmvsTextureSlot,
        archive: &CmvsArchive,
    ) -> Result<Option<TextureFrame>, CoreError> {
        self.bindings.retain(|slot, _| match slot {
            CmvsTextureSlot::Parent(parent) => vm
                .texture_parents
                .get(parent)
                .is_some_and(|state| state.resource.is_some()),
            CmvsTextureSlot::Child { parent, child } => vm
                .texture_children
                .get(parent)
                .and_then(|children| children.get(child))
                .is_some_and(|state| state.resource.is_some()),
        });
        if !self.bindings.contains_key(&requested) {
            return match requested {
                CmvsTextureSlot::Child { .. } => Ok(None),
                CmvsTextureSlot::Parent(_) => {
                    Err(error("ASTRA_EMU_CMVS_TEXTURE_PRESENTATION_BINDING"))
                }
            };
        }
        let geometry = stage_geometry(vm, self.bindings.keys().copied(), requested)?;
        let width = geometry
            .iter()
            .map(|(_, r, _)| r.x as u32 + r.width)
            .max()
            .unwrap();
        let height = geometry
            .iter()
            .map(|(_, r, _)| r.y as u32 + r.height)
            .max()
            .unwrap();
        let mut commands = Vec::with_capacity(geometry.len());
        for (slot, destination, _) in geometry {
            let uri = &self.bindings[&slot];
            let frame = match self.textures.get(uri) {
                Some(frame) => frame,
                None => {
                    let image = archive.decode_pb_image(uri)?;
                    let frame = TextureFrame {
                        width: image.width(),
                        height: image.height(),
                        rgba8: image.into_raw().into(),
                    };
                    self.textures
                        .insert(uri.clone(), frame.clone())
                        .map_err(|_| error("ASTRA_EMU_CMVS_TEXTURE_BUDGET"))?;
                    frame
                }
            };
            commands.push(SceneCommand::Texture {
                id: slot_id(slot),
                frame,
                destination,
                opacity: 1.0,
                blend: BlendMode::Alpha,
            });
        }
        self.draw(width, height, commands).map(Some)
    }

    fn draw(
        &mut self,
        width: u32,
        height: u32,
        commands: Vec<SceneCommand>,
    ) -> Result<TextureFrame, CoreError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_CMVS_FRAME_SEQUENCE"))?;
        let output = self
            .renderer
            .render(&SceneFrame {
                sequence,
                width,
                height,
                clear_rgba: [0, 0, 0, 255],
                commands,
                semantics: None,
            })
            .map_err(|cause| {
                tracing::error!(event = "cmvs.gpu.failed", operation = %cause.operation, code = ?cause.code);
                error("ASTRA_EMU_CMVS_GPU_RENDER")
            })?;
        self.sequence = sequence;
        Ok(TextureFrame {
            width,
            height,
            rgba8: output.rgba8.to_vec().into(),
        })
    }
}

type SlotState = (bool, Option<[u32; 4]>, Option<[u32; 2]>, Option<u32>);
fn slot_state(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> Option<SlotState> {
    match slot {
        CmvsTextureSlot::Parent(parent) => vm.texture_parents.get(&parent).map(|s| {
            (
                s.surface_initialized,
                s.rect_words,
                s.position_words,
                s.auxiliary_word,
            )
        }),
        CmvsTextureSlot::Child { parent, child } => {
            vm.texture_children.get(&parent)?.get(&child).map(|s| {
                (
                    s.surface_initialized,
                    s.rect_words,
                    s.position_words,
                    s.auxiliary_word,
                )
            })
        }
    }
}
fn slot_id(slot: CmvsTextureSlot) -> String {
    match slot {
        CmvsTextureSlot::Parent(p) => format!("cmvs.texture.parent.{p}"),
        CmvsTextureSlot::Child { parent, child } => format!("cmvs.texture.child.{parent}.{child}"),
    }
}
fn stage_geometry(
    vm: &CmvsPs2aVmState,
    slots: impl Iterator<Item = CmvsTextureSlot>,
    requested: CmvsTextureSlot,
) -> Result<Vec<(CmvsTextureSlot, RectI, u32)>, CoreError> {
    let mut geometry = Vec::new();
    for slot in slots {
        let Some((true, Some(rect), Some(position), Some(z))) = slot_state(vm, slot) else {
            continue;
        };
        let bounds = [
            rect[0].checked_add(position[0]),
            rect[1].checked_add(position[1]),
            rect[2].checked_add(position[0]),
            rect[3].checked_add(position[1]),
        ];
        let [Some(x0), Some(y0), Some(x1), Some(y1)] = bounds else {
            return Err(error("ASTRA_EMU_CMVS_TEXTURE_GEOMETRY"));
        };
        if x1 <= x0 || y1 <= y0 || x1 > 16384 || y1 > 16384 || z > i32::MAX as u32 {
            return Err(error("ASTRA_EMU_CMVS_TEXTURE_GEOMETRY"));
        }
        geometry.push((
            slot,
            RectI {
                x: x0 as i32,
                y: y0 as i32,
                width: x1 - x0,
                height: y1 - y0,
            },
            z,
        ));
    }
    if !geometry.iter().any(|(slot, _, _)| *slot == requested) {
        return Err(error("ASTRA_EMU_CMVS_TEXTURE_PRESENTATION_STATE"));
    }
    geometry.sort_by_key(|(slot, _, z)| (*z, *slot));
    Ok(geometry)
}
fn error(code: &'static str) -> CoreError {
    CoreError::invalid(code, "CMVS scene operation failed")
}
