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
pub struct CosmicTextLayoutProvider {
    headless: bool,
}

impl CosmicTextLayoutProvider {
    pub fn new_headless() -> Self {
        Self { headless: true }
    }
}

impl TextLayoutProvider for CosmicTextLayoutProvider {
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError> {
        let _metrics =
            cosmic_text::Metrics::new(request.constraint.font_size, request.constraint.line_height);
        let mut diagnostics = Vec::new();
        if request.constraint.max_width <= 0.0
            || request.constraint.font_size <= 0.0
            || request.constraint.line_height <= 0.0
        {
            return Err(MediaError::message(
                "text layout constraint values must be positive",
            ));
        }
        if self.headless
            && request
                .font_families
                .iter()
                .any(|family| family.to_ascii_lowercase().contains("missing"))
        {
            diagnostics.push(Diagnostic::warning(
                "ASTRA_TEXT_FONT_FALLBACK",
                "requested font family was not available in headless layout",
            ));
        }

        let mut boxes = Vec::new();
        let mut ruby_boxes = Vec::new();
        let mut voice_refs = Vec::new();
        let mut line_cursor = 0u32;
        let char_width = request.constraint.font_size * 0.5;
        for (run_index, run) in request.runs.iter().enumerate() {
            if let Some(voice) = &run.voice {
                voice_refs.push(VoiceReplayRefRecord {
                    asset: voice.asset.clone(),
                    cue: voice.cue.clone(),
                });
            }
            let chars_per_line =
                (request.constraint.max_width / char_width).floor().max(1.0) as usize;
            let chars: Vec<char> = run.text.chars().collect();
            for (line_offset, chunk) in chars.chunks(chars_per_line).enumerate() {
                let text: String = chunk.iter().collect();
                let line = line_cursor + line_offset as u32;
                let char_start = line_offset * chars_per_line;
                let char_end = char_start + chunk.len();
                boxes.push(LayoutBox {
                    run_index,
                    line,
                    x: 0.0,
                    y: line as f32 * request.constraint.line_height,
                    width: (text.chars().count() as f32 * char_width)
                        .min(request.constraint.max_width),
                    height: request.constraint.line_height,
                    text: text.clone(),
                });
                for ruby in &run.ruby {
                    if ruby.base_range.start >= ruby.base_range.end
                        || ruby.base_range.end > chars.len()
                    {
                        diagnostics.push(Diagnostic::blocking(
                            "ASTRA_TEXT_RUBY_RANGE",
                            "ruby base range is outside the text run",
                        ));
                        continue;
                    }
                    let start = ruby.base_range.start.max(char_start);
                    let end = ruby.base_range.end.min(char_end);
                    if start < end {
                        ruby_boxes.push(RubyLayoutBox {
                            run_index,
                            base_start: ruby.base_range.start,
                            base_end: ruby.base_range.end,
                            line,
                            x: (start - char_start) as f32 * char_width,
                            y: line as f32 * request.constraint.line_height
                                - request.constraint.font_size * 0.45,
                            width: (end - start) as f32 * char_width,
                            height: request.constraint.font_size * 0.5,
                            text: ruby.text.clone(),
                        });
                    }
                }
            }
            line_cursor += chars.len().div_ceil(chars_per_line) as u32;
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
