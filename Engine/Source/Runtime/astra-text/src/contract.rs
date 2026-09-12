use astra_core::{Diagnostic, Hash256};
use astra_media_core::GlyphBitmap;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutProviderIdentity {
    pub context: FontBindingContext,
    pub fonts: Vec<PackagedFontIdentity>,
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
    VerticalRightToLeft,
    VerticalLeftToRight,
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
    /// Clockwise quarter turns already applied to the glyph bitmap.
    pub rotation_quadrants: u8,
    /// True when a short horizontal digit run occupies one vertical em cell.
    pub tate_chu_yoko: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq)]
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
    /// Provider-local immutable layout revision. This is assigned once on a
    /// cache miss and reused by every cache hit; it is not a content digest.
    pub revision: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutMeasurement {
    pub width: f32,
    pub height: f32,
    pub clipped: bool,
    pub ellipsized: bool,
    pub revision: u64,
}

pub trait TextLayoutProvider {
    fn identity(&self) -> Result<TextLayoutProviderIdentity, MediaError>;
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError>;
    fn measure(&self, request: &TextLayoutRequest) -> Result<TextLayoutMeasurement, MediaError> {
        Ok(TextLayoutMeasurement::from(&self.layout(request)?))
    }
}

impl From<&TextLayoutResult> for TextLayoutMeasurement {
    fn from(result: &TextLayoutResult) -> Self {
        Self {
            width: result.width,
            height: result.height,
            clipped: result.clipped,
            ellipsized: result.ellipsized,
            revision: result.revision,
        }
    }
}
