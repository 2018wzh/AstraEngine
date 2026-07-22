use cosmic_text::{
    Attrs, Buffer, Ellipsize, EllipsizeHeightLimit, Family, FontFeatures, FontSystem, Metrics,
    Shaping, SwashContent, Wrap,
};
use unicode_segmentation::UnicodeSegmentation;

use astra_media_core::{GlyphBitmap, GlyphBitmapFormat};

use crate::MediaError;

use super::{
    contract::*,
    provider::{FontState, LayoutPass, RawLayout, RawLine},
    validation::{cluster_covered, source_line_offsets, validate_direction},
};

#[derive(Debug, Clone, Copy)]
struct SelectedSpan {
    start: usize,
    end: usize,
    family_index: usize,
}

pub(super) fn append_shaped_glyph(
    runs: &mut Vec<ShapedGlyphRun>,
    run_index: usize,
    role: GlyphRole,
    glyph: ShapedGlyph,
) {
    if let Some(last) = runs.last_mut() {
        if last.run_index == run_index
            && last.role == role
            && last.line == glyph.line
            && last.direction == glyph.direction
            && last.font_asset_id == glyph.font_asset_id
            && last.font_face_index == glyph.font_face_index
            && last.font_hash == glyph.font_hash
        {
            last.glyphs.push(glyph);
            return;
        }
    }
    runs.push(ShapedGlyphRun {
        run_index,
        role,
        line: glyph.line,
        direction: glyph.direction,
        font_asset_id: glyph.font_asset_id.clone(),
        font_family: glyph.font_family.clone(),
        font_face_index: glyph.font_face_index,
        font_hash: glyph.font_hash,
        baseline: glyph.baseline,
        glyphs: vec![glyph],
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn shape_raw(
    state: &mut FontState,
    text: &str,
    locale: &str,
    direction: TextDirection,
    families: &[String],
    features: &FontFeatures,
    constraint: LayoutConstraint,
    config: &TextLayoutConfig,
) -> Result<RawLayout, MediaError> {
    let selected_spans = select_family_spans(text, families, state)?;
    if !state.font_systems.contains_key(locale) {
        if state.font_systems.len() == config.max_locales {
            return Err(MediaError::message(
                "ASTRA_TEXT_LOCALE_BUDGET: active locale count exceeds the configured budget",
            ));
        }
        state.font_systems.insert(
            locale.to_string(),
            FontSystem::new_with_locale_and_db(locale.to_string(), state.database.clone()),
        );
    }
    let system = state.font_systems.get_mut(locale).ok_or_else(|| {
        MediaError::message("ASTRA_TEXT_FONT_SYSTEM: locale font system creation failed")
    })?;
    let preflight = layout_pass(
        system,
        text,
        families,
        &selected_spans,
        features,
        direction,
        constraint,
        false,
    )?;
    let overflowed = detect_overflow(&preflight, constraint);
    let lines = layout_pass(
        system,
        text,
        families,
        &selected_spans,
        features,
        direction,
        constraint,
        true,
    )?
    .lines;
    Ok(RawLayout {
        lines,
        overflowed,
        locale: locale.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn layout_pass(
    system: &mut FontSystem,
    text: &str,
    families: &[String],
    selected_spans: &[SelectedSpan],
    features: &FontFeatures,
    direction: TextDirection,
    constraint: LayoutConstraint,
    apply_overflow: bool,
) -> Result<LayoutPass, MediaError> {
    let metrics = Metrics::new(constraint.font_size, constraint.line_height);
    let mut buffer = Buffer::new(system, metrics);
    let mut buffer = buffer.borrow_with(system);
    buffer.set_wrap(match constraint.wrap {
        WrapPolicy::None => Wrap::None,
        WrapPolicy::Glyph => Wrap::Glyph,
        WrapPolicy::Word => Wrap::Word,
        WrapPolicy::WordOrGlyph => Wrap::WordOrGlyph,
    });
    let height = if apply_overflow {
        constraint.max_height.or_else(|| {
            constraint
                .max_lines
                .map(|lines| lines as f32 * constraint.line_height)
        })
    } else {
        None
    };
    buffer.set_size(Some(constraint.max_width), height);
    if apply_overflow {
        let limit = constraint
            .max_lines
            .map(|lines| EllipsizeHeightLimit::Lines(lines as usize))
            .or_else(|| constraint.max_height.map(EllipsizeHeightLimit::Height))
            .unwrap_or(EllipsizeHeightLimit::Lines(1));
        buffer.set_ellipsize(match constraint.overflow {
            OverflowPolicy::EllipsisStart => Ellipsize::Start(limit),
            OverflowPolicy::EllipsisMiddle => Ellipsize::Middle(limit),
            OverflowPolicy::EllipsisEnd => Ellipsize::End(limit),
            OverflowPolicy::Visible | OverflowPolicy::Clip => Ellipsize::None,
        });
    }
    let default_attrs = Attrs::new()
        .family(Family::Name(&families[0]))
        .font_features(features.clone());
    if text.is_empty() {
        buffer.set_text(text, &default_attrs, Shaping::Advanced, None);
    } else {
        let rich_spans = selected_spans.iter().map(|span| {
            (
                &text[span.start..span.end],
                Attrs::new()
                    .family(Family::Name(&families[span.family_index]))
                    .font_features(features.clone()),
            )
        });
        buffer.set_rich_text(rich_spans, &default_attrs, Shaping::Advanced, None);
    }
    let line_offsets = source_line_offsets(text);
    let lines = buffer
        .layout_runs()
        .map(|layout| {
            let source_offset = *line_offsets.get(layout.line_i).ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_LINE_MAPPING: layout line has no source mapping")
            })?;
            validate_direction(direction, layout.rtl)?;
            let cluster_start = layout
                .glyphs
                .iter()
                .map(|glyph| glyph.start)
                .min()
                .unwrap_or(0);
            let cluster_end = layout
                .glyphs
                .iter()
                .map(|glyph| glyph.end)
                .max()
                .unwrap_or(layout.text.len());
            Ok(RawLine {
                source: SourceRange {
                    start: source_offset + cluster_start,
                    end: source_offset + cluster_end,
                },
                rtl: layout.rtl,
                top: layout.line_top,
                baseline: layout.line_y,
                width: layout.line_w,
                height: layout.line_height,
                glyphs: layout.glyphs.to_vec(),
                source_offset,
            })
        })
        .collect::<Result<Vec<_>, MediaError>>()?;
    let total_lines = buffer
        .lines
        .iter()
        .map(|line| line.layout_opt().map_or(0, Vec::len))
        .sum::<usize>();
    let max_width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    Ok(LayoutPass {
        lines,
        total_lines,
        max_width,
    })
}

fn select_family_spans(
    text: &str,
    families: &[String],
    state: &FontState,
) -> Result<Vec<SelectedSpan>, MediaError> {
    let mut spans: Vec<SelectedSpan> = Vec::new();
    for (start, grapheme) in text.grapheme_indices(true) {
        let family_index = families
            .iter()
            .position(|family| {
                state.faces.values().any(|face| {
                    face.family.eq_ignore_ascii_case(family)
                        && cluster_covered(grapheme, &face.coverage)
                })
            })
            .ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_GLYPH_MISSING: no declared fallback coverage contains a grapheme cluster",
                )
            })?;
        let end = start + grapheme.len();
        if let Some(previous) = spans.last_mut() {
            if previous.end == start && previous.family_index == family_index {
                previous.end = end;
                continue;
            }
        }
        spans.push(SelectedSpan {
            start,
            end,
            family_index,
        });
    }
    Ok(spans)
}

fn detect_overflow(pass: &LayoutPass, constraint: LayoutConstraint) -> bool {
    let width_overflow =
        constraint.wrap == WrapPolicy::None && pass.max_width > constraint.max_width;
    let line_overflow = constraint
        .max_lines
        .is_some_and(|max_lines| pass.total_lines > max_lines as usize);
    let height_overflow = constraint
        .max_height
        .is_some_and(|height| pass.total_lines as f32 * constraint.line_height > height);
    width_overflow || line_overflow || height_overflow
}

pub(super) fn glyph_bitmap(image: &cosmic_text::SwashImage) -> Result<GlyphBitmap, MediaError> {
    let pixels = (image.placement.width as usize)
        .checked_mul(image.placement.height as usize)
        .ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_GLYPH_BITMAP: rasterized dimensions overflow")
        })?;
    if image.placement.width == 0 || image.placement.height == 0 {
        return Err(MediaError::message(format!(
            "ASTRA_TEXT_GLYPH_BITMAP: rasterizer returned an invalid bitmap (content={:?}, width={}, height={}, bytes={})",
            image.content,
            image.placement.width,
            image.placement.height,
            image.data.len(),
        )));
    }
    let (format, pixels) = match image.content {
        SwashContent::Mask if image.data.len() == pixels => {
            (GlyphBitmapFormat::Alpha8, image.data.clone())
        }
        SwashContent::SubpixelMask
            if image.data.len()
                == pixels.checked_mul(3).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_GLYPH_BITMAP: subpixel bitmap size overflows")
                })? =>
        {
            // SceneCommand uses target-independent glyph resources. Collapse RGB subpixel
            // coverage to a stable mask so replay does not depend on panel stripe order.
            let alpha = image
                .data
                .chunks_exact(3)
                .map(|coverage| coverage[0].max(coverage[1]).max(coverage[2]))
                .collect();
            (GlyphBitmapFormat::Alpha8, alpha)
        }
        SwashContent::Color
            if image.data.len()
                == pixels.checked_mul(4).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_GLYPH_BITMAP: color bitmap size overflows")
                })? =>
        {
            (GlyphBitmapFormat::Rgba8, image.data.clone())
        }
        _ => {
            return Err(MediaError::message(format!(
                "ASTRA_TEXT_GLYPH_BITMAP: rasterizer returned an invalid bitmap (content={:?}, width={}, height={}, bytes={})",
                image.content,
                image.placement.width,
                image.placement.height,
                image.data.len(),
            )))
        }
    };
    GlyphBitmap::from_pixels(
        image.placement.width,
        image.placement.height,
        format,
        pixels.into(),
    )
}
