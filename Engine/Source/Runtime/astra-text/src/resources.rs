use crate::{LayoutClip, MediaError, TextLayoutResult, TEXT_LAYOUT_SCHEMA};
use astra_media_core::{BlendMode, GlyphBitmap, GlyphInstance, RectI, SceneCommand};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

impl TextLayoutResult {
    fn glyph_instances(&self) -> Vec<GlyphInstance> {
        self.shaped_runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(|glyph| {
                Some(GlyphInstance {
                    resource_id: glyph.resource_id.clone()?,
                    x: glyph.render_x?,
                    y: glyph.render_y?,
                    rotation_quadrants: glyph.rotation_quadrants,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct OwnedGlyphResource {
    bitmap: GlyphBitmap,
    references: usize,
    last_used_sequence: u64,
}

#[derive(Debug, Clone)]
struct OwnedTextLayout {
    resource_ids: BTreeSet<String>,
    layout_revision: u64,
    rgba: [u8; 4],
    translation: (i32, i32),
    commands: Vec<SceneCommand>,
    shared_layout: Option<Arc<TextLayoutResult>>,
}

/// Host-owned bridge from immutable shaped layouts to renderer resource lifetime commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RetainedGlyphCachePolicy {
    max_resources: usize,
    max_bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TextRenderResourceOwner {
    resources: BTreeMap<String, OwnedGlyphResource>,
    layouts: BTreeMap<String, OwnedTextLayout>,
    retained_cache: Option<RetainedGlyphCachePolicy>,
    usage_sequence: u64,
}

pub struct TextRenderLayoutUpdate<'a> {
    pub layout_id: &'a str,
    pub layout: &'a TextLayoutResult,
    /// Optional host-owned immutable layout identity. Reusing this exact `Arc`
    /// permits the resource owner to replay validated draw commands without
    /// rescanning glyph payloads. A borrowed/mutable layout always takes the
    /// full validation path.
    pub shared_layout: Option<&'a Arc<TextLayoutResult>>,
    pub rgba: [u8; 4],
    pub translation: (i32, i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRenderLayoutDraw {
    pub layout_id: String,
    pub commands: Vec<SceneCommand>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRenderFrameCommands {
    pub lifecycle: Vec<SceneCommand>,
    pub layouts: Vec<TextRenderLayoutDraw>,
}

struct PreparedTextLayout {
    layout_id: String,
    next_ids: BTreeSet<String>,
    glyphs: Vec<GlyphInstance>,
    clip: Option<LayoutClip>,
    rgba: [u8; 4],
    translation: (i32, i32),
    layout_revision: u64,
    cached_commands: Option<Vec<SceneCommand>>,
    shared_layout: Option<Arc<TextLayoutResult>>,
}

impl TextRenderResourceOwner {
    /// Invalidates renderer-owned glyph residency after a device loss.
    ///
    /// Logical layout results remain owned by the caller and will be submitted
    /// again on the next frame. Clearing both maps is required because cached
    /// draw commands are only valid while their glyph resources remain live.
    pub fn invalidate_device_resources(&mut self) -> usize {
        let resource_count = self.resources.len();
        self.layouts.clear();
        self.resources.clear();
        self.usage_sequence = 0;
        resource_count
    }

    /// Creates a resource owner that keeps unreferenced, content-addressed glyphs
    /// in a deterministic bounded LRU. This is intended for product hosts where
    /// dialogue and system pages repeatedly reuse the same packaged fonts.
    pub fn with_retained_glyph_cache(
        max_resources: usize,
        max_bytes: usize,
    ) -> Result<Self, MediaError> {
        if max_resources == 0 || max_bytes == 0 {
            return Err(MediaError::message(
                "ASTRA_TEXT_RENDER_CACHE_BUDGET: retained glyph limits must be non-zero",
            ));
        }
        Ok(Self {
            retained_cache: Some(RetainedGlyphCachePolicy {
                max_resources,
                max_bytes,
            }),
            ..Self::default()
        })
    }

    pub fn update_frame(
        &mut self,
        updates: &[TextRenderLayoutUpdate<'_>],
        removals: &[&str],
    ) -> Result<TextRenderFrameCommands, MediaError> {
        let mut affected_layouts = BTreeSet::new();
        let mut prepared = Vec::with_capacity(updates.len());
        let mut new_bitmaps = BTreeMap::new();
        let mut reference_changes = BTreeMap::<String, (usize, usize)>::new();

        for update in updates {
            if update.layout_id.trim().is_empty() || update.layout.schema != TEXT_LAYOUT_SCHEMA {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_BINDING: layout id or schema is invalid",
                ));
            }
            if !affected_layouts.insert(update.layout_id) {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_LAYOUT_DUPLICATE: frame updates a layout more than once",
                ));
            }
            if let Some(shared_layout) = update.shared_layout {
                if !std::ptr::eq(shared_layout.as_ref(), update.layout) {
                    return Err(MediaError::message(
                        "ASTRA_TEXT_RENDER_SHARED_LAYOUT_IDENTITY: shared layout does not own the borrowed layout",
                    ));
                }
            }
            if let (Some(previous), Some(shared_layout)) =
                (self.layouts.get(update.layout_id), update.shared_layout)
            {
                if previous.layout_revision == update.layout.revision
                    && previous.rgba == update.rgba
                    && previous.translation == update.translation
                    && previous
                        .shared_layout
                        .as_ref()
                        .is_some_and(|owned| Arc::ptr_eq(owned, shared_layout))
                {
                    self.validate_layout_resources(&previous.resource_ids)?;
                    prepared.push(PreparedTextLayout {
                        layout_id: update.layout_id.to_string(),
                        next_ids: previous.resource_ids.clone(),
                        glyphs: Vec::new(),
                        clip: None,
                        rgba: update.rgba,
                        translation: update.translation,
                        layout_revision: update.layout.revision,
                        cached_commands: Some(previous.commands.clone()),
                        shared_layout: update.shared_layout.cloned(),
                    });
                    continue;
                }
            }
            let declared = update
                .layout
                .glyph_resources
                .iter()
                .map(|resource| (resource.resource_id.as_str(), resource))
                .collect::<BTreeMap<_, _>>();
            if declared.len() != update.layout.glyph_resources.len() {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_RESOURCE_DUPLICATE: layout contains duplicate glyph resources",
                ));
            }
            let mut glyphs = update.layout.glyph_instances();
            for glyph in &mut glyphs {
                glyph.x = glyph.x.checked_add(update.translation.0).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_RENDER_TRANSLATION: glyph x overflowed")
                })?;
                glyph.y = glyph.y.checked_add(update.translation.1).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_RENDER_TRANSLATION: glyph y overflowed")
                })?;
            }
            let next_ids = glyphs
                .iter()
                .map(|instance| instance.resource_id.clone())
                .collect::<BTreeSet<_>>();
            if next_ids
                .iter()
                .any(|id| !declared.contains_key(id.as_str()))
            {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_RESOURCE_MISSING: glyph instance has no declared bitmap",
                ));
            }
            for id in &next_ids {
                let bitmap = &declared
                    .get(id.as_str())
                    .expect("next resource was validated")
                    .bitmap;
                if let Some(owned) = self.resources.get(id) {
                    if owned.bitmap != *bitmap {
                        return Err(MediaError::message(
                            "ASTRA_TEXT_RENDER_RESOURCE_CONFLICT: content-addressed glyph id changed bitmap",
                        ));
                    }
                } else if let Some(previous) = new_bitmaps.get(id) {
                    if previous != bitmap {
                        return Err(MediaError::message(
                            "ASTRA_TEXT_RENDER_RESOURCE_CONFLICT: frame declares conflicting glyph bitmaps",
                        ));
                    }
                } else {
                    new_bitmaps.insert(id.clone(), bitmap.clone());
                }
            }

            let previous_ids = self
                .layouts
                .get(update.layout_id)
                .map(|layout| layout.resource_ids.clone())
                .unwrap_or_default();
            self.validate_layout_resources(&previous_ids)?;
            for id in previous_ids.difference(&next_ids) {
                increment_reference_change(&mut reference_changes, id, true)?;
            }
            for id in next_ids.difference(&previous_ids) {
                increment_reference_change(&mut reference_changes, id, false)?;
            }
            prepared.push(PreparedTextLayout {
                layout_id: update.layout_id.to_string(),
                next_ids,
                glyphs,
                clip: update
                    .layout
                    .clip
                    .map(|clip| translate_layout_clip(clip, update.translation))
                    .transpose()?,
                rgba: update.rgba,
                translation: update.translation,
                layout_revision: update.layout.revision,
                cached_commands: None,
                shared_layout: update.shared_layout.cloned(),
            });
        }

        for layout_id in removals {
            if layout_id.trim().is_empty() || !affected_layouts.insert(*layout_id) {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_LAYOUT_DUPLICATE: frame removes an invalid or updated layout",
                ));
            }
            let layout = self.layouts.get(*layout_id).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_RENDER_LAYOUT_UNKNOWN: layout is not bound")
            })?;
            self.validate_layout_resources(&layout.resource_ids)?;
            for id in &layout.resource_ids {
                increment_reference_change(&mut reference_changes, id, true)?;
            }
        }

        let mut next_reference_counts = BTreeMap::new();
        for (id, (removed, added)) in &reference_changes {
            let current = self
                .resources
                .get(id)
                .map_or(0, |resource| resource.references);
            let next = current
                .checked_sub(*removed)
                .and_then(|value| value.checked_add(*added))
                .ok_or_else(|| {
                    MediaError::message(
                        "ASTRA_TEXT_RENDER_STATE: glyph reference count overflow or underflow",
                    )
                })?;
            if current == 0
                && next > 0
                && !self.resources.contains_key(id)
                && !new_bitmaps.contains_key(id)
            {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: new glyph resource has no declared bitmap",
                ));
            }
            next_reference_counts.insert(id.clone(), next);
        }

        let next_usage_sequence = self.usage_sequence.checked_add(1).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_RENDER_STATE: glyph usage sequence overflowed")
        })?;
        let mut lifecycle = Vec::new();
        for (id, next) in &next_reference_counts {
            if *next == 0 {
                if self.retained_cache.is_some() {
                    let resource = self.resources.get_mut(id).ok_or_else(|| {
                        MediaError::message(
                            "ASTRA_TEXT_RENDER_STATE: released glyph resource is not owned",
                        )
                    })?;
                    resource.references = 0;
                    resource.last_used_sequence = next_usage_sequence;
                } else {
                    self.resources.remove(id);
                    lifecycle.push(SceneCommand::ReleaseResource {
                        resource_id: id.clone(),
                    });
                }
            } else if let Some(resource) = self.resources.get_mut(id) {
                resource.references = *next;
                resource.last_used_sequence = next_usage_sequence;
            } else {
                let bitmap = new_bitmaps
                    .remove(id)
                    .expect("new resource bitmap was prevalidated");
                self.resources.insert(
                    id.clone(),
                    OwnedGlyphResource {
                        bitmap: bitmap.clone(),
                        references: *next,
                        last_used_sequence: next_usage_sequence,
                    },
                );
                lifecycle.push(SceneCommand::UploadGlyph {
                    resource_id: id.clone(),
                    glyph: bitmap,
                });
            }
        }
        self.usage_sequence = next_usage_sequence;
        self.evict_dormant_glyphs(&mut lifecycle)?;
        for layout_id in removals {
            self.layouts.remove(*layout_id);
        }

        let mut layouts = Vec::with_capacity(prepared.len());
        for layout in prepared {
            if let Some(commands) = layout.cached_commands {
                layouts.push(TextRenderLayoutDraw {
                    layout_id: layout.layout_id,
                    commands,
                });
                continue;
            }
            let mut commands = Vec::with_capacity(3);
            if let Some(clip) = layout.clip {
                commands.push(SceneCommand::PushClip {
                    rect: RectI::new(clip.x, clip.y, clip.width, clip.height),
                });
            }
            commands.push(SceneCommand::GlyphRun {
                id: layout.layout_id.clone(),
                glyphs: layout.glyphs.into(),
                rgba: layout.rgba,
                opacity: 1.0,
                blend: BlendMode::Alpha,
            });
            if layout.clip.is_some() {
                commands.push(SceneCommand::PopClip);
            }
            self.layouts.insert(
                layout.layout_id.clone(),
                OwnedTextLayout {
                    resource_ids: layout.next_ids,
                    layout_revision: layout.layout_revision,
                    rgba: layout.rgba,
                    translation: layout.translation,
                    commands: commands.clone(),
                    shared_layout: layout.shared_layout,
                },
            );
            layouts.push(TextRenderLayoutDraw {
                layout_id: layout.layout_id,
                commands,
            });
        }
        tracing::debug!(
            target: "astra_text",
            event = "text.render_resources.frame_updated",
            layout_count = self.layouts.len(),
            resource_count = self.resources.len(),
            lifecycle_count = lifecycle.len(),
            draw_count = layouts.len(),
        );
        Ok(TextRenderFrameCommands { lifecycle, layouts })
    }

    pub fn update_layout(
        &mut self,
        layout_id: &str,
        layout: &TextLayoutResult,
        rgba: [u8; 4],
    ) -> Result<Vec<SceneCommand>, MediaError> {
        let frame = self.update_frame(
            &[TextRenderLayoutUpdate {
                layout_id,
                layout,
                shared_layout: None,
                rgba,
                translation: (0, 0),
            }],
            &[],
        )?;
        let mut commands = frame.lifecycle;
        commands.extend(
            frame
                .layouts
                .into_iter()
                .next()
                .expect("single layout update must produce a draw")
                .commands,
        );
        Ok(commands)
    }

    pub fn remove_layout(&mut self, layout_id: &str) -> Result<Vec<SceneCommand>, MediaError> {
        Ok(self.update_frame(&[], &[layout_id])?.lifecycle)
    }

    fn validate_layout_resources(&self, ids: &BTreeSet<String>) -> Result<(), MediaError> {
        for id in ids {
            let resource = self.resources.get(id).ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: layout referenced an unowned glyph resource",
                )
            })?;
            if resource.references == 0 {
                return Err(MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: glyph resource has no references",
                ));
            }
        }
        Ok(())
    }

    fn evict_dormant_glyphs(
        &mut self,
        lifecycle: &mut Vec<SceneCommand>,
    ) -> Result<(), MediaError> {
        let Some(policy) = self.retained_cache else {
            return Ok(());
        };
        let mut dormant = self
            .resources
            .iter()
            .filter(|(_, resource)| resource.references == 0)
            .map(|(id, resource)| {
                (
                    resource.last_used_sequence,
                    id.clone(),
                    resource.bitmap.pixels.len(),
                )
            })
            .collect::<Vec<_>>();
        dormant.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
        let mut dormant_count = dormant.len();
        let mut dormant_bytes = dormant.iter().try_fold(0usize, |total, entry| {
            total.checked_add(entry.2).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_RENDER_CACHE_BUDGET: dormant byte count overflowed")
            })
        })?;
        let mut evicted_count = 0usize;
        let mut evicted_bytes = 0usize;
        for (_, id, byte_size) in dormant {
            if dormant_count <= policy.max_resources && dormant_bytes <= policy.max_bytes {
                break;
            }
            self.resources.remove(&id).ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: dormant glyph disappeared during eviction",
                )
            })?;
            dormant_count -= 1;
            dormant_bytes -= byte_size;
            evicted_count += 1;
            evicted_bytes = evicted_bytes.checked_add(byte_size).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_RENDER_CACHE_BUDGET: eviction bytes overflowed")
            })?;
            lifecycle.push(SceneCommand::ReleaseResource { resource_id: id });
        }
        if evicted_count > 0 {
            tracing::debug!(
                target: "astra_text",
                event = "text.render_resources.cache_evicted",
                evicted_count,
                evicted_bytes,
                dormant_count,
                dormant_bytes,
            );
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Vec<SceneCommand> {
        let commands: Vec<SceneCommand> = self
            .resources
            .keys()
            .map(|resource_id| SceneCommand::ReleaseResource {
                resource_id: resource_id.clone(),
            })
            .collect();
        self.resources.clear();
        self.layouts.clear();
        tracing::info!(
            target: "astra_text",
            event = "text.render_resources.shutdown",
            release_count = commands.len(),
        );
        commands
    }
}

fn translate_layout_clip(
    mut clip: LayoutClip,
    translation: (i32, i32),
) -> Result<LayoutClip, MediaError> {
    clip.x = clip
        .x
        .checked_add(translation.0)
        .ok_or_else(|| MediaError::message("ASTRA_TEXT_RENDER_TRANSLATION: clip x overflowed"))?;
    clip.y = clip
        .y
        .checked_add(translation.1)
        .ok_or_else(|| MediaError::message("ASTRA_TEXT_RENDER_TRANSLATION: clip y overflowed"))?;
    Ok(clip)
}

fn increment_reference_change(
    changes: &mut BTreeMap<String, (usize, usize)>,
    resource_id: &str,
    remove: bool,
) -> Result<(), MediaError> {
    let change = changes.entry(resource_id.to_string()).or_default();
    let value = if remove { &mut change.0 } else { &mut change.1 };
    *value = value.checked_add(1).ok_or_else(|| {
        MediaError::message("ASTRA_TEXT_RENDER_STATE: frame reference journal overflow")
    })?;
    Ok(())
}
