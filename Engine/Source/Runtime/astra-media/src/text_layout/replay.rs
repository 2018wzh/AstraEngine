use std::collections::BTreeSet;

use astra_core::{is_safe_symbol as safe_symbol, Hash256};

use astra_media_core::GlyphBitmapFormat;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use crate::MediaError;

use super::{
    TextLayoutProvider, TextLayoutProviderIdentity, TextLayoutRequest, TextLayoutResult,
    TEXT_LAYOUT_SCHEMA,
};

mod binary_text_layout {
    use astra_core::{Diagnostic, DiagnosticSeverity, Hash256, SourceSpan};
    use astra_media_core::{GlyphBitmap, GlyphBitmapFormat, OwnedPixelBuffer};
    use schemars::JsonSchema;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::super::{
        GlyphResource, GlyphRole, LayoutClip, LayoutLine, RubyLayoutBox, ShapedGlyph,
        ShapedGlyphRun, SourceRange, TextDirection, TextLayoutResult, VoiceReplayRefRecord,
    };

    #[derive(Serialize, Deserialize, JsonSchema)]
    pub(super) struct LayoutWire {
        schema: String,
        key: String,
        width: f32,
        height: f32,
        lines: Vec<LineWire>,
        shaped_runs: Vec<RunWire>,
        glyph_resources: Vec<GlyphResourceWire>,
        ruby_boxes: Vec<RubyLayoutBox>,
        voice_refs: Vec<VoiceReplayRefRecord>,
        clip: Option<LayoutClip>,
        clipped: bool,
        ellipsized: bool,
        diagnostics: Vec<DiagnosticWire>,
        revision: u64,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct LineWire {
        run_index: usize,
        role: RoleWire,
        line: u32,
        source: SourceRange,
        rtl: bool,
        top: f32,
        baseline: f32,
        width: f32,
        height: f32,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct RunWire {
        run_index: usize,
        role: RoleWire,
        line: u32,
        direction: TextDirection,
        font_asset_id: String,
        font_family: String,
        font_face_index: u32,
        font_hash: Hash256,
        baseline: f32,
        glyphs: Vec<ShapedGlyph>,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    enum RoleWire {
        Base,
        Ruby(usize),
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct GlyphResourceWire {
        resource_id: String,
        font_asset_id: String,
        font_hash: Hash256,
        glyph_id: u16,
        bitmap: GlyphBitmapWire,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct GlyphBitmapWire {
        width: u32,
        height: u32,
        format: GlyphBitmapFormat,
        pixels: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct DiagnosticWire {
        severity: DiagnosticSeverity,
        code: String,
        message: String,
        source: Option<SourceSpan>,
        fields: std::collections::BTreeMap<String, String>,
    }

    pub fn serialize<S>(layout: &TextLayoutResult, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        LayoutWire::from(layout).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<TextLayoutResult, D::Error>
    where
        D: Deserializer<'de>,
    {
        LayoutWire::deserialize(deserializer).map(TextLayoutResult::from)
    }

    impl From<&TextLayoutResult> for LayoutWire {
        fn from(layout: &TextLayoutResult) -> Self {
            Self {
                schema: layout.schema.clone(),
                key: layout.key.clone(),
                width: layout.width,
                height: layout.height,
                lines: layout.lines.iter().map(LineWire::from).collect(),
                shaped_runs: layout.shaped_runs.iter().map(RunWire::from).collect(),
                glyph_resources: layout
                    .glyph_resources
                    .iter()
                    .map(GlyphResourceWire::from)
                    .collect(),
                ruby_boxes: layout.ruby_boxes.clone(),
                voice_refs: layout.voice_refs.clone(),
                clip: layout.clip,
                clipped: layout.clipped,
                ellipsized: layout.ellipsized,
                diagnostics: layout
                    .diagnostics
                    .iter()
                    .map(DiagnosticWire::from)
                    .collect(),
                revision: layout.revision,
            }
        }
    }

    impl From<LayoutWire> for TextLayoutResult {
        fn from(layout: LayoutWire) -> Self {
            Self {
                schema: layout.schema,
                key: layout.key,
                width: layout.width,
                height: layout.height,
                lines: layout.lines.into_iter().map(LayoutLine::from).collect(),
                shaped_runs: layout
                    .shaped_runs
                    .into_iter()
                    .map(ShapedGlyphRun::from)
                    .collect(),
                glyph_resources: layout
                    .glyph_resources
                    .into_iter()
                    .map(GlyphResource::from)
                    .collect(),
                ruby_boxes: layout.ruby_boxes,
                voice_refs: layout.voice_refs,
                clip: layout.clip,
                clipped: layout.clipped,
                ellipsized: layout.ellipsized,
                diagnostics: layout
                    .diagnostics
                    .into_iter()
                    .map(Diagnostic::from)
                    .collect(),
                revision: layout.revision,
            }
        }
    }

    impl From<&LayoutLine> for LineWire {
        fn from(line: &LayoutLine) -> Self {
            Self {
                run_index: line.run_index,
                role: RoleWire::from(&line.role),
                line: line.line,
                source: line.source.clone(),
                rtl: line.rtl,
                top: line.top,
                baseline: line.baseline,
                width: line.width,
                height: line.height,
            }
        }
    }

    impl From<LineWire> for LayoutLine {
        fn from(line: LineWire) -> Self {
            Self {
                run_index: line.run_index,
                role: GlyphRole::from(line.role),
                line: line.line,
                source: line.source,
                rtl: line.rtl,
                top: line.top,
                baseline: line.baseline,
                width: line.width,
                height: line.height,
            }
        }
    }

    impl From<&ShapedGlyphRun> for RunWire {
        fn from(run: &ShapedGlyphRun) -> Self {
            Self {
                run_index: run.run_index,
                role: RoleWire::from(&run.role),
                line: run.line,
                direction: run.direction,
                font_asset_id: run.font_asset_id.clone(),
                font_family: run.font_family.clone(),
                font_face_index: run.font_face_index,
                font_hash: run.font_hash,
                baseline: run.baseline,
                glyphs: run.glyphs.clone(),
            }
        }
    }

    impl From<RunWire> for ShapedGlyphRun {
        fn from(run: RunWire) -> Self {
            Self {
                run_index: run.run_index,
                role: GlyphRole::from(run.role),
                line: run.line,
                direction: run.direction,
                font_asset_id: run.font_asset_id,
                font_family: run.font_family,
                font_face_index: run.font_face_index,
                font_hash: run.font_hash,
                baseline: run.baseline,
                glyphs: run.glyphs,
            }
        }
    }

    impl From<&GlyphRole> for RoleWire {
        fn from(role: &GlyphRole) -> Self {
            match role {
                GlyphRole::Base => Self::Base,
                GlyphRole::Ruby { span_index } => Self::Ruby(*span_index),
            }
        }
    }

    impl From<RoleWire> for GlyphRole {
        fn from(role: RoleWire) -> Self {
            match role {
                RoleWire::Base => Self::Base,
                RoleWire::Ruby(span_index) => Self::Ruby { span_index },
            }
        }
    }

    impl From<&GlyphResource> for GlyphResourceWire {
        fn from(resource: &GlyphResource) -> Self {
            Self {
                resource_id: resource.resource_id.clone(),
                font_asset_id: resource.font_asset_id.clone(),
                font_hash: resource.font_hash,
                glyph_id: resource.glyph_id,
                bitmap: GlyphBitmapWire::from(&resource.bitmap),
            }
        }
    }

    impl From<GlyphResourceWire> for GlyphResource {
        fn from(resource: GlyphResourceWire) -> Self {
            Self {
                resource_id: resource.resource_id,
                font_asset_id: resource.font_asset_id,
                font_hash: resource.font_hash,
                glyph_id: resource.glyph_id,
                bitmap: GlyphBitmap::from(resource.bitmap),
            }
        }
    }

    impl From<&GlyphBitmap> for GlyphBitmapWire {
        fn from(bitmap: &GlyphBitmap) -> Self {
            Self {
                width: bitmap.width,
                height: bitmap.height,
                format: bitmap.format,
                pixels: bitmap.pixels.as_slice().to_vec(),
            }
        }
    }

    impl From<GlyphBitmapWire> for GlyphBitmap {
        fn from(bitmap: GlyphBitmapWire) -> Self {
            Self {
                width: bitmap.width,
                height: bitmap.height,
                format: bitmap.format,
                pixels: OwnedPixelBuffer::from_vec(bitmap.pixels),
            }
        }
    }

    impl From<&Diagnostic> for DiagnosticWire {
        fn from(diagnostic: &Diagnostic) -> Self {
            Self {
                severity: diagnostic.severity,
                code: diagnostic.code.clone(),
                message: diagnostic.message.clone(),
                source: diagnostic.source.clone(),
                fields: diagnostic.fields.clone(),
            }
        }
    }

    impl From<DiagnosticWire> for Diagnostic {
        fn from(diagnostic: DiagnosticWire) -> Self {
            Self {
                severity: diagnostic.severity,
                code: diagnostic.code,
                message: diagnostic.message,
                source: diagnostic.source,
                fields: diagnostic.fields,
            }
        }
    }
}

mod replay_contract;
pub use replay_contract::text_layout_evidence_hash;
pub use replay_contract::text_layout_replay_request_hash;
pub use replay_contract::TextLayoutBindingIdentity;
pub use replay_contract::TextLayoutReplayInput;
pub use replay_contract::TextLayoutReplayLimits;
pub use replay_contract::TextLayoutReplayRecord;
pub use replay_contract::TextLayoutReplaySession;
pub use replay_contract::TextLayoutReplaySnapshot;
pub use replay_contract::TEXT_LAYOUT_REPLAY_SCHEMA;
pub use replay_contract::TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA;
mod validate_layout;
use validate_layout::binding_hash;
use validate_layout::canonical_hash;
use validate_layout::ensure_snapshot_budget;
use validate_layout::record_hash;
use validate_layout::safe_language;
use validate_layout::safe_resource_id;
use validate_layout::snapshot_hash;
use validate_layout::text_replay_error;
use validate_layout::transcript_hash;
use validate_layout::valid_coverage;
use validate_layout::validate_layout;
use validate_layout::PersistedLayoutRef;
