use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::{Diagnostic, Hash256};
use cosmic_text::{fontdb, FontFeatures};

use crate::MediaError;

use super::{
    contract::*,
    provider::{FontState, LoadedDatabase, LoadedFace},
};

pub(super) fn load_database(
    context: &FontBindingContext,
    config: &TextLayoutConfig,
    fonts: &[PackagedFont],
) -> Result<LoadedDatabase, MediaError> {
    if fonts.is_empty() {
        return Err(MediaError::message(
            "ASTRA_TEXT_PACKAGED_FONT_MISSING: font database requires packaged font bytes",
        ));
    }
    if fonts.len() > config.max_fonts {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_BUDGET: packaged font count exceeds the configured budget",
        ));
    }
    let total_bytes = fonts.iter().try_fold(0usize, |total, font| {
        total.checked_add(font.bytes.len()).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_FONT_BUDGET: packaged font byte count overflows")
        })
    })?;
    if total_bytes > config.max_font_bytes {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_BUDGET: packaged font bytes exceed the configured budget",
        ));
    }
    let mut assets = BTreeSet::new();
    let mut face_bindings = BTreeSet::new();
    let mut by_hash = BTreeMap::<Hash256, Vec<&PackagedFont>>::new();
    for font in fonts {
        validate_font(context, font)?;
        if !assets.insert(font.asset_id.clone()) {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_DUPLICATE: packaged font asset id is duplicated",
            ));
        }
        if !face_bindings.insert((font.hash, font.face_index)) {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_FACE_DUPLICATE: packaged font hash and face index are duplicated",
            ));
        }
        by_hash.entry(font.hash).or_default().push(font);
    }
    let mut database = fontdb::Database::new();
    let mut faces = BTreeMap::new();
    for (hash, descriptors) in by_hash {
        let ids = database.load_font_source(fontdb::Source::Binary(Arc::new(
            descriptors[0].bytes.clone(),
        )));
        if ids.is_empty() {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_PARSE: packaged font contains no readable face",
            ));
        }
        let selected = descriptors
            .iter()
            .map(|font| font.face_index)
            .collect::<BTreeSet<_>>();
        for id in ids {
            let info = database.face(id).cloned().ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_FONT_PARSE: loaded face metadata is missing")
            })?;
            if !selected.contains(&info.index) {
                database.remove_face(id);
                continue;
            }
            let descriptor = descriptors
                .iter()
                .find(|font| font.face_index == info.index)
                .ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_FONT_FACE: selected face descriptor is missing")
                })?;
            if !info
                .families
                .iter()
                .any(|family| family.0.eq_ignore_ascii_case(&descriptor.family))
            {
                return Err(MediaError::message(
                    "ASTRA_TEXT_FONT_FAMILY: declared family does not match packaged face metadata",
                ));
            }
            faces.insert(
                id.to_string(),
                LoadedFace {
                    asset_id: descriptor.asset_id.clone(),
                    family: descriptor.family.clone(),
                    face_index: descriptor.face_index,
                    hash,
                    coverage: descriptor.coverage.clone(),
                },
            );
        }
    }
    if faces.len() != fonts.len() {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_FACE: not every packaged font descriptor resolved to one face",
        ));
    }
    Ok(LoadedDatabase { database, faces })
}

pub(super) fn validate_context(context: &FontBindingContext) -> Result<(), MediaError> {
    if !safe_identity(&context.target)
        || !safe_identity(&context.profile)
        || !safe_language(&context.default_locale)
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_BINDING_CONTEXT: target, profile, or locale is invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_config(config: &TextLayoutConfig) -> Result<(), MediaError> {
    if config.max_fonts == 0
        || config.max_font_bytes == 0
        || config.max_text_bytes == 0
        || config.max_runs == 0
        || config.max_ruby_spans == 0
        || config.max_locales == 0
        || config.max_glyphs == 0
        || config.max_cache_entries == 0
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_CONFIG: every text layout budget must be non-zero",
        ));
    }
    Ok(())
}

fn validate_font(context: &FontBindingContext, font: &PackagedFont) -> Result<(), MediaError> {
    if !font.asset_id.starts_with("asset:/")
        || font.asset_id.contains("..")
        || font.asset_id.contains('\\')
        || font.family.trim().is_empty()
        || font.family.len() > 128
        || !safe_identity(&font.license_id)
        || font
            .subset
            .as_ref()
            .is_some_and(|value| !safe_identity(value))
        || font.coverage.is_empty()
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_DESCRIPTOR: packaged font descriptor is invalid",
        ));
    }
    if font.bytes.is_empty() || Hash256::from_sha256(&font.bytes) != font.hash {
        return Err(MediaError::message(
            "ASTRA_TEXT_PACKAGED_FONT_HASH: package font bytes do not match the descriptor hash",
        ));
    }
    if font.targets.is_empty()
        || font.profiles.is_empty()
        || font.targets.iter().any(|value| !safe_identity(value))
        || font.profiles.iter().any(|value| !safe_identity(value))
        || contains_duplicate_identity(&font.targets)
        || contains_duplicate_identity(&font.profiles)
        || !font.targets.iter().any(|target| target == &context.target)
        || !font
            .profiles
            .iter()
            .any(|profile| profile == &context.profile)
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_ELIGIBILITY: packaged font target or profile is invalid",
        ));
    }
    let mut previous_end = None;
    for range in &font.coverage {
        if range.start > range.end
            || range.end > char::MAX as u32
            || previous_end.is_some_and(|end| range.start <= end)
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_COVERAGE: coverage ranges must be ordered, disjoint Unicode scalar ranges",
            ));
        }
        previous_end = Some(range.end);
    }
    Ok(())
}

pub(super) fn validate_request(
    request: &TextLayoutRequest,
    config: &TextLayoutConfig,
) -> Result<(), MediaError> {
    if request.key.trim().is_empty() {
        return Err(MediaError::message(
            "ASTRA_TEXT_REQUEST_KEY: layout request key is empty",
        ));
    }
    if request.runs.len() > config.max_runs
        || request.runs.iter().map(|run| run.ruby.len()).sum::<usize>() > config.max_ruby_spans
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_RUN_BUDGET: text run or ruby span count exceeds the configured budget",
        ));
    }
    let values = [
        request.constraint.max_width,
        request.constraint.font_size,
        request.constraint.line_height,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || request
            .constraint
            .max_height
            .is_some_and(|value| !value.is_finite() || value <= 0.0)
        || request.constraint.max_lines == Some(0)
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_LAYOUT_CONSTRAINT: layout constraints must be finite and positive",
        ));
    }
    if request.constraint.max_width > u32::MAX as f32
        || request
            .constraint
            .max_height
            .is_some_and(|value| value > u32::MAX as f32)
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_LAYOUT_CONSTRAINT: layout dimensions exceed renderer coordinates",
        ));
    }
    if request.constraint.font_size > request.constraint.line_height * 4.0 {
        return Err(MediaError::message(
            "ASTRA_TEXT_LAYOUT_CONSTRAINT: font size is incompatible with line height",
        ));
    }
    if request.font_families.is_empty()
        || request
            .font_families
            .iter()
            .any(|family| family.trim().is_empty() || family.len() > 128)
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_CHAIN: explicit font family chain is required",
        ));
    }
    let mut unique_families = BTreeSet::new();
    if request
        .font_families
        .iter()
        .any(|family| !unique_families.insert(family.to_ascii_lowercase()))
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_CHAIN: font family chain contains duplicates",
        ));
    }
    let text_bytes = request.runs.iter().try_fold(0usize, |total, run| {
        total.checked_add(run.text.len()).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_INPUT_BUDGET: text byte count overflows")
        })
    })?;
    if text_bytes > config.max_text_bytes {
        return Err(MediaError::message(
            "ASTRA_TEXT_INPUT_BUDGET: text exceeds the configured byte budget",
        ));
    }
    for run in &request.runs {
        if !safe_language(&run.language)
            || run
                .script
                .as_ref()
                .is_some_and(|script| !safe_script(script))
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_LANGUAGE: language or script declaration is invalid",
            ));
        }
        if let Some(voice) = &run.voice {
            if !voice.asset.starts_with("asset:/")
                || voice.asset.contains("..")
                || voice.cue.trim().is_empty()
            {
                return Err(MediaError::message(
                    "ASTRA_TEXT_VOICE_REF: voice replay reference is invalid",
                ));
            }
        }
        for ruby in &run.ruby {
            validate_ruby(run, ruby)?;
        }
    }
    cosmic_features(&request.features)?;
    Ok(())
}

pub(super) fn validate_family_chain(
    request: &TextLayoutRequest,
    state: &FontState,
) -> Result<(), MediaError> {
    for family in &request.font_families {
        if !state
            .faces
            .values()
            .any(|face| face.family.eq_ignore_ascii_case(family))
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_CHAIN: declared font family is not installed",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_ruby(run: &TextRun, ruby: &RubySpan) -> Result<(), MediaError> {
    if ruby.base_range.start >= ruby.base_range.end
        || ruby.base_range.end > run.text.len()
        || !run.text.is_char_boundary(ruby.base_range.start)
        || !run.text.is_char_boundary(ruby.base_range.end)
        || ruby.text.is_empty()
    {
        return Err(MediaError::message(
            "ASTRA_TEXT_RUBY_RANGE: ruby range must be a non-empty UTF-8 byte range",
        ));
    }
    Ok(())
}

pub(super) fn validate_direction(
    direction: TextDirection,
    actual_rtl: bool,
) -> Result<(), MediaError> {
    let mismatch = matches!(direction, TextDirection::LeftToRight) && actual_rtl
        || matches!(direction, TextDirection::RightToLeft) && !actual_rtl;
    if mismatch {
        return Err(MediaError::message(
            "ASTRA_TEXT_DIRECTION: declared paragraph direction disagrees with Unicode bidi shaping",
        ));
    }
    Ok(())
}

pub(super) fn cosmic_features(features: &[OpenTypeFeature]) -> Result<FontFeatures, MediaError> {
    let mut result = FontFeatures::new();
    let mut seen = BTreeSet::new();
    for feature in features {
        let bytes: [u8; 4] = feature.tag.as_bytes().try_into().map_err(|_| {
            MediaError::message("ASTRA_TEXT_FEATURE: feature tag must be four bytes")
        })?;
        if !bytes.iter().all(u8::is_ascii_alphanumeric) || !seen.insert(bytes) {
            return Err(MediaError::message(
                "ASTRA_TEXT_FEATURE: feature tag is invalid or duplicated",
            ));
        }
        result.set(cosmic_text::FeatureTag::new(&bytes), feature.value);
    }
    Ok(result)
}

pub(super) fn request_cache_key(
    request: &TextLayoutRequest,
    fonts: &[PackagedFont],
) -> Result<Hash256, MediaError> {
    let identities = fonts
        .iter()
        .map(PackagedFontIdentity::from)
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(TEXT_LAYOUT_SCHEMA, request, identities))
        .map_err(|error| MediaError::message(error.to_string()))?;
    Ok(Hash256::from_sha256(&bytes))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn result_hash(
    request: &TextLayoutRequest,
    context: &FontBindingContext,
    fonts: &[PackagedFont],
    width: f32,
    height: f32,
    lines: &[LayoutLine],
    shaped_runs: &[ShapedGlyphRun],
    resources: &[GlyphResource],
    ruby_boxes: &[RubyLayoutBox],
    voice_refs: &[VoiceReplayRefRecord],
    clip: Option<LayoutClip>,
    clipped: bool,
    ellipsized: bool,
    diagnostics: &[Diagnostic],
) -> Result<Hash256, MediaError> {
    let identities = fonts
        .iter()
        .map(PackagedFontIdentity::from)
        .collect::<Vec<_>>();
    let resource_identities = resources
        .iter()
        .map(|resource| {
            (
                &resource.resource_id,
                &resource.font_asset_id,
                resource.font_hash,
                resource.glyph_id,
                resource.bitmap.width,
                resource.bitmap.height,
                &resource.bitmap.format,
                resource.bitmap.hash,
            )
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(
        TEXT_LAYOUT_SCHEMA,
        request,
        context,
        identities,
        width,
        height,
        lines,
        shaped_runs,
        resource_identities,
        ruby_boxes,
        voice_refs,
        clip,
        clipped,
        ellipsized,
        diagnostics,
    ))
    .map_err(|error| MediaError::message(error.to_string()))?;
    Ok(Hash256::from_sha256(&bytes))
}

pub(super) fn source_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

pub(super) fn ranges_overlap(left: &SourceRange, right: &SourceRange) -> bool {
    left.start < right.end && right.start < left.end
}

pub(super) fn cluster_covered(cluster: &str, coverage: &[UnicodeRange]) -> bool {
    cluster.chars().all(|value| {
        value.is_control()
            || coverage
                .iter()
                .any(|range| range.start <= value as u32 && value as u32 <= range.end)
    })
}

fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn safe_script(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn contains_duplicate_identity(values: &[String]) -> bool {
    let mut unique = BTreeSet::new();
    values.iter().any(|value| !unique.insert(value))
}
