use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash256};
use astra_media_core::{BlendMode, GlyphBitmap, GlyphInstance, RectI, SceneCommand};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

pub const TEXT_LAYOUT_SCHEMA: &str = "astra.text_layout.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontBindingContext {
    pub target: String,
    pub profile: String,
    pub default_locale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutConfig {
    pub max_fonts: usize,
    pub max_font_bytes: usize,
    pub max_text_bytes: usize,
    pub max_runs: usize,
    pub max_ruby_spans: usize,
    pub max_locales: usize,
    pub max_glyphs: usize,
    pub max_cache_entries: usize,
}

impl TextLayoutConfig {
    pub const fn production_defaults() -> Self {
        Self {
            max_fonts: 64,
            max_font_bytes: 64 * 1024 * 1024,
            max_text_bytes: 1024 * 1024,
            max_runs: 4096,
            max_ruby_spans: 16_384,
            max_locales: 32,
            max_glyphs: 262_144,
            max_cache_entries: 512,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutRequest {
    pub key: String,
    pub runs: Vec<TextRun>,
    pub constraint: LayoutConstraint,
    /// Ordered and explicit fallback chain. Every face used by shaping must be in this list.
    pub font_families: Vec<String>,
    #[serde(default)]
    pub features: Vec<OpenTypeFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedFont {
    pub asset_id: String,
    pub family: String,
    pub face_index: u32,
    pub hash: Hash256,
    pub license_id: String,
    pub subset: Option<String>,
    pub coverage: Vec<UnicodeRange>,
    pub targets: Vec<String>,
    pub profiles: Vec<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PackagedFontIdentity {
    pub asset_id: String,
    pub family: String,
    pub face_index: u32,
    pub hash: Hash256,
    pub license_id: String,
    pub subset: Option<String>,
    pub coverage: Vec<UnicodeRange>,
}

impl From<&PackagedFont> for PackagedFontIdentity {
    fn from(font: &PackagedFont) -> Self {
        Self {
            asset_id: font.asset_id.clone(),
            family: font.family.clone(),
            face_index: font.face_index,
            hash: font.hash,
            license_id: font.license_id.clone(),
            subset: font.subset.clone(),
            coverage: font.coverage.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnicodeRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextRun {
    pub text: String,
    pub language: String,
    pub script: Option<String>,
    pub direction: TextDirection,
    #[serde(default)]
    pub ruby: Vec<RubySpan>,
    pub voice: Option<VoiceReplayRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Auto,
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RubySpan {
    /// UTF-8 byte range in the base run.
    pub base_range: SourceRange,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceReplayRef {
    pub asset: String,
    pub cue: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpenTypeFeature {
    /// Four-byte OpenType feature tag, for example `kern` or `liga`.
    pub tag: String,
    pub value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WrapPolicy {
    None,
    Glyph,
    Word,
    WordOrGlyph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    Visible,
    Clip,
    EllipsisStart,
    EllipsisMiddle,
    EllipsisEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutConstraint {
    pub max_width: f32,
    pub max_height: Option<f32>,
    pub max_lines: Option<u32>,
    pub font_size: f32,
    pub line_height: f32,
    pub wrap: WrapPolicy,
    pub overflow: OverflowPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutLine {
    pub run_index: usize,
    pub role: GlyphRole,
    pub line: u32,
    pub source: SourceRange,
    pub rtl: bool,
    pub top: f32,
    pub baseline: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GlyphRole {
    Base,
    Ruby { span_index: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapedGlyph {
    pub source: SourceRange,
    pub glyph_id: u16,
    pub font_asset_id: String,
    pub font_family: String,
    pub font_face_index: u32,
    pub font_hash: Hash256,
    pub direction: TextDirection,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub baseline: f32,
    pub line: u32,
    pub resource_id: Option<String>,
    pub render_x: Option<i32>,
    pub render_y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapedGlyphRun {
    pub run_index: usize,
    pub role: GlyphRole,
    pub line: u32,
    pub direction: TextDirection,
    pub font_asset_id: String,
    pub font_family: String,
    pub font_face_index: u32,
    pub font_hash: Hash256,
    pub baseline: f32,
    pub glyphs: Vec<ShapedGlyph>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlyphResource {
    pub resource_id: String,
    pub font_asset_id: String,
    pub font_hash: Hash256,
    pub glyph_id: u16,
    pub bitmap: GlyphBitmap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RubyLayoutBox {
    pub run_index: usize,
    pub span_index: usize,
    pub base_range: SourceRange,
    pub line: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutClip {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceReplayRefRecord {
    pub run_index: usize,
    pub asset: String,
    pub cue: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutResult {
    pub schema: String,
    pub key: String,
    pub width: f32,
    pub height: f32,
    pub lines: Vec<LayoutLine>,
    pub shaped_runs: Vec<ShapedGlyphRun>,
    pub glyph_resources: Vec<GlyphResource>,
    pub ruby_boxes: Vec<RubyLayoutBox>,
    pub voice_refs: Vec<VoiceReplayRefRecord>,
    pub clip: Option<LayoutClip>,
    pub clipped: bool,
    pub ellipsized: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub hash: Hash256,
}

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
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct OwnedGlyphResource {
    bitmap: GlyphBitmap,
    references: usize,
}

/// Host-owned bridge from immutable shaped layouts to renderer resource lifetime commands.
#[derive(Debug, Clone, Default)]
pub struct TextRenderResourceOwner {
    resources: BTreeMap<String, OwnedGlyphResource>,
    layouts: BTreeMap<String, BTreeSet<String>>,
}

impl TextRenderResourceOwner {
    pub fn update_layout(
        &mut self,
        layout_id: &str,
        layout: &TextLayoutResult,
        rgba: [u8; 4],
    ) -> Result<Vec<SceneCommand>, MediaError> {
        if layout_id.trim().is_empty() || layout.schema != TEXT_LAYOUT_SCHEMA {
            return Err(MediaError::message(
                "ASTRA_TEXT_RENDER_BINDING: layout id or schema is invalid",
            ));
        }
        let declared = layout
            .glyph_resources
            .iter()
            .map(|resource| (resource.resource_id.clone(), resource))
            .collect::<BTreeMap<_, _>>();
        if declared.len() != layout.glyph_resources.len() {
            return Err(MediaError::message(
                "ASTRA_TEXT_RENDER_RESOURCE_DUPLICATE: layout contains duplicate glyph resources",
            ));
        }
        let next_ids = layout
            .glyph_instances()
            .into_iter()
            .map(|instance| instance.resource_id)
            .collect::<BTreeSet<_>>();
        if next_ids.iter().any(|id| !declared.contains_key(id)) {
            return Err(MediaError::message(
                "ASTRA_TEXT_RENDER_RESOURCE_MISSING: glyph instance has no declared bitmap",
            ));
        }
        for id in &next_ids {
            if let Some(owned) = self.resources.get(id) {
                let declared_resource = declared.get(id).expect("next resource was validated");
                if owned.bitmap != declared_resource.bitmap {
                    return Err(MediaError::message(
                        "ASTRA_TEXT_RENDER_RESOURCE_CONFLICT: content-addressed glyph id changed bitmap",
                    ));
                }
            }
        }

        let mut resources = self.resources.clone();
        let mut layouts = self.layouts.clone();
        let previous_ids = layouts.remove(layout_id).unwrap_or_default();
        let mut commands = Vec::new();
        for id in previous_ids.difference(&next_ids) {
            let resource = resources.get_mut(id).ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: layout referenced an unowned glyph resource",
                )
            })?;
            resource.references = resource.references.checked_sub(1).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_RENDER_STATE: glyph reference count underflow")
            })?;
            if resource.references == 0 {
                resources.remove(id);
                commands.push(SceneCommand::ReleaseResource {
                    resource_id: id.clone(),
                });
            }
        }
        for id in next_ids.difference(&previous_ids) {
            let declared_resource = declared.get(id).expect("next resource was validated");
            if let Some(resource) = resources.get_mut(id) {
                if resource.bitmap != declared_resource.bitmap {
                    return Err(MediaError::message(
                        "ASTRA_TEXT_RENDER_RESOURCE_CONFLICT: content-addressed glyph id changed bitmap",
                    ));
                }
                resource.references = resource.references.checked_add(1).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_RENDER_STATE: glyph reference count overflow")
                })?;
            } else {
                resources.insert(
                    id.clone(),
                    OwnedGlyphResource {
                        bitmap: declared_resource.bitmap.clone(),
                        references: 1,
                    },
                );
                commands.push(SceneCommand::UploadGlyph {
                    resource_id: id.clone(),
                    glyph: declared_resource.bitmap.clone(),
                });
            }
        }
        layouts.insert(layout_id.to_string(), next_ids);
        if let Some(clip) = layout.clip {
            commands.push(SceneCommand::PushClip {
                rect: RectI::new(clip.x, clip.y, clip.width, clip.height),
            });
        }
        commands.push(SceneCommand::GlyphRun {
            id: layout_id.to_string(),
            glyphs: layout.glyph_instances(),
            rgba,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
        if layout.clip.is_some() {
            commands.push(SceneCommand::PopClip);
        }
        self.resources = resources;
        self.layouts = layouts;
        tracing::debug!(
            target: "astra_media::text",
            event = "text.render_resources.updated",
            layout_count = self.layouts.len(),
            resource_count = self.resources.len(),
            command_count = commands.len(),
        );
        Ok(commands)
    }

    pub fn remove_layout(&mut self, layout_id: &str) -> Result<Vec<SceneCommand>, MediaError> {
        let mut resources = self.resources.clone();
        let mut layouts = self.layouts.clone();
        let ids = layouts.remove(layout_id).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_RENDER_LAYOUT_UNKNOWN: layout is not bound")
        })?;
        let mut commands = Vec::new();
        for id in ids {
            let resource = resources.get_mut(&id).ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_RENDER_STATE: layout referenced an unowned glyph resource",
                )
            })?;
            resource.references = resource.references.checked_sub(1).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_RENDER_STATE: glyph reference count underflow")
            })?;
            if resource.references == 0 {
                resources.remove(&id);
                commands.push(SceneCommand::ReleaseResource { resource_id: id });
            }
        }
        self.resources = resources;
        self.layouts = layouts;
        tracing::debug!(
            target: "astra_media::text",
            event = "text.render_resources.layout_removed",
            layout_count = self.layouts.len(),
            resource_count = self.resources.len(),
            release_count = commands.len(),
        );
        Ok(commands)
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
            target: "astra_media::text",
            event = "text.render_resources.shutdown",
            release_count = commands.len(),
        );
        commands
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutCacheStats {
    pub font_generation: u64,
    pub font_count: usize,
    pub face_count: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}

pub trait TextLayoutProvider {
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError>;
    fn layout_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError>;
}
