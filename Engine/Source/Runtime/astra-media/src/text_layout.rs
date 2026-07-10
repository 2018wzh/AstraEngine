use std::ops::Range;

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayoutRequest {
    pub key: String,
    pub runs: Vec<TextRun>,
    pub constraint: LayoutConstraint,
    pub font_families: Vec<String>,
    pub fonts: Vec<PackagedFont>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedFont {
    pub asset_id: String,
    pub family: String,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub locale: String,
    pub ruby: Vec<RubySpan>,
    pub voice: Option<VoiceReplayRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RubySpan {
    pub base_range: Range<usize>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceReplayRef {
    pub asset: String,
    pub cue: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutConstraint {
    pub max_width: f32,
    pub font_size: f32,
    pub line_height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutBox {
    pub run_index: usize,
    pub line: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutResult {
    pub boxes: Vec<LayoutBox>,
    pub ruby_boxes: Vec<RubyLayoutBox>,
    pub voice_refs: Vec<VoiceReplayRefRecord>,
    pub diagnostics: Vec<Diagnostic>,
    pub hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RubyLayoutBox {
    pub run_index: usize,
    pub base_start: usize,
    pub base_end: usize,
    pub line: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceReplayRefRecord {
    pub asset: String,
    pub cue: String,
}

pub trait TextLayoutProvider {
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError>;
    fn layout_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError>;
}

#[derive(Debug, Clone, Default)]
pub struct CosmicTextLayoutProvider;

impl CosmicTextLayoutProvider {
    pub fn new_headless() -> Self {
        Self
    }
}

impl TextLayoutProvider for CosmicTextLayoutProvider {
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError> {
        let mut diagnostics = Vec::new();
        if request.constraint.max_width <= 0.0
            || request.constraint.font_size <= 0.0
            || request.constraint.line_height <= 0.0
        {
            return Err(MediaError::message(
                "text layout constraint values must be positive",
            ));
        }
        if request.fonts.is_empty() {
            return Err(MediaError::message(
                "ASTRA_TEXT_PACKAGED_FONT_MISSING: text layout requires package font bytes",
            ));
        }

        let locale = request
            .runs
            .first()
            .map(|run| run.locale.clone())
            .unwrap_or_else(|| "und".into());
        let mut font_system = cosmic_text::FontSystem::new_with_locale_and_db(
            locale,
            cosmic_text::fontdb::Database::new(),
        );
        for font in &request.fonts {
            if font.bytes.is_empty() || Hash256::from_sha256(&font.bytes) != font.hash {
                return Err(MediaError::message(
                    "ASTRA_TEXT_PACKAGED_FONT_HASH: package font bytes do not match the sidecar hash",
                ));
            }
            font_system.db_mut().load_font_data(font.bytes.clone());
        }
        for family in &request.font_families {
            if !request.fonts.iter().any(|font| font.family == *family) {
                diagnostics.push(
                    Diagnostic::warning(
                        "ASTRA_TEXT_FONT_FALLBACK",
                        "requested package font family was unavailable; packaged fallback was used",
                    )
                    .with_field("family", family),
                );
            }
        }

        let mut boxes = Vec::new();
        let mut ruby_boxes = Vec::new();
        let mut voice_refs = Vec::new();
        let mut line_cursor = 0u32;
        for (run_index, run) in request.runs.iter().enumerate() {
            if let Some(voice) = &run.voice {
                voice_refs.push(VoiceReplayRefRecord {
                    asset: voice.asset.clone(),
                    cue: voice.cue.clone(),
                });
            }
            let metrics = cosmic_text::Metrics::new(
                request.constraint.font_size,
                request.constraint.line_height,
            );
            let mut buffer = cosmic_text::Buffer::new(&mut font_system, metrics);
            let mut buffer = buffer.borrow_with(&mut font_system);
            buffer.set_size(Some(request.constraint.max_width), None);
            let family = request
                .font_families
                .first()
                .map(String::as_str)
                .filter(|family| request.fonts.iter().any(|font| font.family == **family))
                .unwrap_or_else(|| request.fonts[0].family.as_str());
            let attrs = cosmic_text::Attrs::new().family(cosmic_text::Family::Name(family));
            buffer.set_text(&run.text, &attrs, cosmic_text::Shaping::Advanced, None);
            let shaped = buffer
                .layout_runs()
                .map(|layout| {
                    (
                        layout.line_i,
                        layout.text.to_string(),
                        layout.line_top,
                        layout.line_w,
                        layout.line_height,
                    )
                })
                .collect::<Vec<_>>();
            for (line_offset, (_, text, top, width, height)) in shaped.iter().enumerate() {
                let line = line_cursor + line_offset as u32;
                boxes.push(LayoutBox {
                    run_index,
                    line,
                    x: 0.0,
                    y: line_cursor as f32 * request.constraint.line_height + *top,
                    width: *width,
                    height: *height,
                    text: text.clone(),
                });
            }
            let char_count = run.text.chars().count();
            for ruby in &run.ruby {
                if ruby.base_range.start >= ruby.base_range.end || ruby.base_range.end > char_count
                {
                    diagnostics.push(Diagnostic::blocking(
                        "ASTRA_TEXT_RUBY_RANGE",
                        "ruby base range is outside the text run",
                    ));
                    continue;
                }
                let fraction = ruby.base_range.start as f32 / char_count.max(1) as f32;
                ruby_boxes.push(RubyLayoutBox {
                    run_index,
                    base_start: ruby.base_range.start,
                    base_end: ruby.base_range.end,
                    line: line_cursor,
                    x: boxes
                        .last()
                        .map(|layout| layout.width * fraction)
                        .unwrap_or_default(),
                    y: line_cursor as f32 * request.constraint.line_height
                        - request.constraint.font_size * 0.45,
                    width: request.constraint.font_size
                        * (ruby.base_range.end - ruby.base_range.start) as f32,
                    height: request.constraint.font_size * 0.5,
                    text: ruby.text.clone(),
                });
            }
            line_cursor += shaped.len().max(1) as u32;
        }
        let hash = layout_hash(&boxes, &ruby_boxes, &voice_refs, &diagnostics)?;
        Ok(TextLayoutResult {
            boxes,
            ruby_boxes,
            voice_refs,
            diagnostics,
            hash,
        })
    }

    fn layout_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError> {
        Ok(self.layout(request)?.hash)
    }
}

fn layout_hash(
    boxes: &[LayoutBox],
    ruby_boxes: &[RubyLayoutBox],
    voice_refs: &[VoiceReplayRefRecord],
    diagnostics: &[Diagnostic],
) -> Result<Hash256, MediaError> {
    let payload = serde_json::to_vec(&(boxes, ruby_boxes, voice_refs, diagnostics))
        .map_err(|err| MediaError::message(err.to_string()))?;
    Ok(Hash256::from_sha256(&payload))
}
