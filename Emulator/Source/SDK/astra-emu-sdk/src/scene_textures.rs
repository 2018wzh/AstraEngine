use astra_media_core::{MediaError, SceneCommand, TextureFrame};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TextureIdentity {
    address: usize,
    bytes: usize,
    width: u32,
    height: u32,
}

struct ResidentTexture {
    resource_id: String,
    seen: u64,
    // Pins the immutable allocation, so a freed address cannot become a cache hit.
    _frame: TextureFrame,
}

/// Persistent uploads for immutable CPU textures used by the current scene.
///
/// One instance belongs to one renderer. Identical owners share one GPU resource;
/// geometry/opacity changes only emit Sprite. Textures no longer drawn are released
/// before the next frame. Thus retained CPU/GPU assets never accumulate with history.
/// Existing renderer resource/byte limits still apply to the live working set.
/// Prepare immediately before rendering; a render failure is terminal for this
/// renderer/cache pair. Reopening or replacing a renderer creates a fresh cache.
#[derive(Default)]
pub struct SceneTextureResidency {
    resources: BTreeMap<TextureIdentity, ResidentTexture>,
    sequence: u64,
    resource_sequence: u64,
    mutations: Vec<SceneCommand>,
}

impl SceneTextureResidency {
    pub fn prepare(&mut self, commands: &mut Vec<SceneCommand>) -> Result<(), MediaError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| MediaError::message("ASTRA_EMU_SDK_TEXTURE_SEQUENCE"))?;
        for command in commands.iter_mut() {
            let SceneCommand::Texture { frame, .. } = command else {
                continue;
            };
            let identity = TextureIdentity {
                address: frame.rgba8.as_ref().as_ptr() as usize,
                bytes: frame.rgba8.len(),
                width: frame.width,
                height: frame.height,
            };
            let resource_id = match self.resources.get_mut(&identity) {
                Some(resource) => {
                    resource.seen = self.sequence;
                    resource.resource_id.clone()
                }
                None => {
                    self.resource_sequence = self
                        .resource_sequence
                        .checked_add(1)
                        .ok_or_else(|| MediaError::message("ASTRA_EMU_SDK_TEXTURE_SEQUENCE"))?;
                    let id = format!("astra.emu.sdk.texture:{}", self.resource_sequence);
                    self.resources.insert(
                        identity,
                        ResidentTexture {
                            resource_id: id.clone(),
                            seen: self.sequence,
                            _frame: frame.clone(),
                        },
                    );
                    self.mutations.push(SceneCommand::UploadTexture {
                        resource_id: id.clone(),
                        frame: frame.clone(),
                    });
                    id
                }
            };
            let SceneCommand::Texture {
                id,
                destination,
                opacity,
                blend,
                ..
            } = std::mem::replace(command, SceneCommand::PopTransform)
            else {
                unreachable!("texture variant checked above");
            };
            *command = SceneCommand::Sprite {
                id,
                texture_id: resource_id,
                source: None,
                destination,
                opacity,
                blend,
            };
        }
        self.resources.retain(|_, resource| {
            if resource.seen == self.sequence {
                return true;
            }
            self.mutations.push(SceneCommand::ReleaseResource {
                resource_id: resource.resource_id.clone(),
            });
            false
        });
        if !self.mutations.is_empty() {
            // Reuse the command allocation. No prepend/memmove per asset and no
            // second command vector allocation on unchanged-resource frames.
            self.mutations.append(commands);
            std::mem::swap(commands, &mut self.mutations);
        }
        Ok(())
    }
}
