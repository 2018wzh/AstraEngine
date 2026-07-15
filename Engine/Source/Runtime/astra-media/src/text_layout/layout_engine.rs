use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash256};
use cosmic_text::FontFeatures;

use crate::MediaError;

use super::{
    contract::*,
    provider::{FontState, RawLayout},
    shaping::{append_shaped_glyph, glyph_bitmap, shape_raw},
    validation::{
        cluster_covered, cosmic_features, ranges_overlap, result_hash, validate_direction,
        validate_ruby,
    },
};

pub(super) fn layout_uncached(
    request: &TextLayoutRequest,
    state: &mut FontState,
    context: &FontBindingContext,
    config: &TextLayoutConfig,
) -> Result<TextLayoutResult, MediaError> {
    let features = cosmic_features(&request.features)?;
    let mut lines = Vec::new();
    let mut shaped_runs = Vec::new();
    let mut resources = BTreeMap::new();
    let mut ruby_boxes = Vec::new();
    let mut voice_refs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut line_cursor = 0u32;
    let mut y_cursor = 0.0f32;
    let mut clipped = false;
    let mut ellipsized = false;

    for (run_index, run) in request.runs.iter().enumerate() {
        if let Some(voice) = &run.voice {
            voice_refs.push(VoiceReplayRefRecord {
                run_index,
                asset: voice.asset.clone(),
                cue: voice.cue.clone(),
            });
        }
        let ruby_padding = if run.ruby.is_empty() {
            0.0
        } else {
            request.constraint.font_size * 0.6
        };
        let raw = shape_raw(
            state,
            &run.text,
            &run.language,
            run.direction,
            &request.font_families,
            &features,
            shaping_constraint(request.constraint, run.direction),
            config,
        )?;
        clipped |= raw.overflowed && request.constraint.overflow == OverflowPolicy::Clip;
        ellipsized |= raw.overflowed
            && matches!(
                request.constraint.overflow,
                OverflowPolicy::EllipsisStart
                    | OverflowPolicy::EllipsisMiddle
                    | OverflowPolicy::EllipsisEnd
            );
        append_raw_layout(
            raw,
            state,
            run_index,
            GlyphRole::Base,
            line_cursor,
            y_cursor + ruby_padding,
            &run.text,
            run.direction,
            &request.font_families,
            &mut lines,
            &mut shaped_runs,
            &mut resources,
            &mut diagnostics,
            config,
        )?;
        let base_line_count = lines
            .iter()
            .filter(|line| line.run_index == run_index && line.role == GlyphRole::Base)
            .count() as u32;
        for (span_index, ruby) in run.ruby.iter().enumerate() {
            append_ruby(
                state,
                request,
                run,
                run_index,
                span_index,
                ruby,
                line_cursor,
                &features,
                &mut lines,
                &mut shaped_runs,
                &mut resources,
                &mut ruby_boxes,
                &mut diagnostics,
                config,
            )?;
        }
        if is_vertical(run.direction) {
            transform_vertical_run(
                run,
                run_index,
                line_cursor,
                request.constraint,
                &mut lines,
                &mut shaped_runs,
                &mut ruby_boxes,
            )?;
        }
        let run_bottom = lines
            .iter()
            .filter(|line| line.run_index == run_index)
            .map(|line| line.top + line.height)
            .fold(y_cursor + ruby_padding, f32::max);
        y_cursor = run_bottom.max(y_cursor + request.constraint.line_height);
        line_cursor = line_cursor
            .checked_add(base_line_count.max(1))
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_LINE_COUNT: layout line count overflow")
            })?;
    }

    let glyph_count = shaped_runs
        .iter()
        .map(|run| run.glyphs.len())
        .sum::<usize>();
    if glyph_count > config.max_glyphs {
        return Err(MediaError::message(
            "ASTRA_TEXT_GLYPH_BUDGET: shaped glyph count exceeds the configured budget",
        ));
    }
    let width = lines
        .iter()
        .map(|line| {
            if request
                .runs
                .get(line.run_index)
                .is_some_and(|run| is_vertical(run.direction))
            {
                (line.baseline - request.constraint.font_size + line.width).max(0.0)
            } else {
                line.width
            }
        })
        .chain(ruby_boxes.iter().map(|ruby| ruby.x + ruby.width))
        .fold(0.0, f32::max);
    let height = lines
        .iter()
        .map(|line| line.top + line.height)
        .chain(ruby_boxes.iter().map(|ruby| ruby.y + ruby.height))
        .fold(0.0, f32::max);
    let glyph_resources = resources.into_values().collect::<Vec<_>>();
    let clip = if request.constraint.overflow == OverflowPolicy::Clip {
        Some(LayoutClip {
            x: 0,
            y: 0,
            width: request.constraint.max_width.ceil() as u32,
            height: request
                .constraint
                .max_height
                .unwrap_or_else(|| {
                    request.constraint.max_lines.map_or(height, |lines| {
                        lines as f32 * request.constraint.line_height
                    })
                })
                .ceil()
                .max(1.0) as u32,
        })
    } else {
        None
    };
    let hash = result_hash(
        request,
        context,
        &state.fonts,
        width,
        height,
        &lines,
        &shaped_runs,
        &glyph_resources,
        &ruby_boxes,
        &voice_refs,
        clip,
        clipped,
        ellipsized,
        &diagnostics,
    )?;
    Ok(TextLayoutResult {
        schema: TEXT_LAYOUT_SCHEMA.to_string(),
        key: request.key.clone(),
        width,
        height,
        lines,
        shaped_runs,
        glyph_resources,
        ruby_boxes,
        voice_refs,
        clip,
        clipped,
        ellipsized,
        diagnostics,
        hash,
    })
}

#[allow(clippy::too_many_arguments)]
fn append_ruby(
    state: &mut FontState,
    request: &TextLayoutRequest,
    run: &TextRun,
    run_index: usize,
    span_index: usize,
    ruby: &RubySpan,
    line_cursor: u32,
    features: &FontFeatures,
    lines: &mut Vec<LayoutLine>,
    shaped_runs: &mut Vec<ShapedGlyphRun>,
    resources: &mut BTreeMap<String, GlyphResource>,
    ruby_boxes: &mut Vec<RubyLayoutBox>,
    diagnostics: &mut Vec<Diagnostic>,
    config: &TextLayoutConfig,
) -> Result<(), MediaError> {
    validate_ruby(run, ruby)?;
    let base_glyphs = shaped_runs
        .iter()
        .filter(|shaped| shaped.run_index == run_index && shaped.role == GlyphRole::Base)
        .flat_map(|shaped| &shaped.glyphs)
        .filter(|glyph| ranges_overlap(&glyph.source, &ruby.base_range))
        .collect::<Vec<_>>();
    if base_glyphs.is_empty() {
        return Err(MediaError::message(
            "ASTRA_TEXT_RUBY_CLUSTER: ruby range did not resolve to a shaped base cluster",
        ));
    }
    let base_lines = base_glyphs
        .iter()
        .map(|glyph| glyph.line)
        .collect::<BTreeSet<_>>();
    if base_lines.len() != 1 {
        return Err(MediaError::message(
            "ASTRA_TEXT_RUBY_MULTILINE: one ruby span cannot cross a wrapped line boundary",
        ));
    }
    let line = *base_lines.first().expect("base line set is non-empty");
    let base_x = base_glyphs
        .iter()
        .map(|glyph| glyph.x)
        .fold(f32::INFINITY, f32::min);
    let base_end = base_glyphs
        .iter()
        .map(|glyph| glyph.x + glyph.advance)
        .fold(f32::NEG_INFINITY, f32::max);
    let base_width = (base_end - base_x).max(0.0);
    let base_top = lines
        .iter()
        .find(|candidate| {
            candidate.run_index == run_index
                && candidate.role == GlyphRole::Base
                && candidate.line == line
        })
        .map(|candidate| candidate.top)
        .ok_or_else(|| MediaError::message("ASTRA_TEXT_RUBY_LINE: base line is missing"))?;
    let ruby_size = request.constraint.font_size * 0.5;
    let ruby_height = request.constraint.line_height * 0.5;
    let ruby_constraint = LayoutConstraint {
        max_width: base_width.max(ruby_size),
        max_height: None,
        max_lines: Some(1),
        font_size: ruby_size,
        line_height: ruby_height,
        wrap: WrapPolicy::None,
        overflow: OverflowPolicy::Visible,
    };
    let raw = shape_raw(
        state,
        &ruby.text,
        &run.language,
        run.direction,
        &request.font_families,
        features,
        ruby_constraint,
        config,
    )?;
    let ruby_width = raw.lines.first().map(|value| value.width).unwrap_or(0.0);
    let ruby_x = base_x + (base_width - ruby_width) * 0.5;
    let ruby_y = (base_top - ruby_height).max(0.0);
    append_raw_layout(
        raw,
        state,
        run_index,
        GlyphRole::Ruby { span_index },
        line,
        ruby_y,
        &ruby.text,
        run.direction,
        &request.font_families,
        lines,
        shaped_runs,
        resources,
        diagnostics,
        config,
    )?;
    for glyph in shaped_runs
        .iter_mut()
        .filter(|shaped| {
            shaped.run_index == run_index
                && shaped.role == GlyphRole::Ruby { span_index }
                && shaped.line == line
        })
        .flat_map(|shaped| &mut shaped.glyphs)
    {
        glyph.x += ruby_x;
        glyph.render_x = glyph.render_x.map(|x| x + ruby_x.round() as i32);
    }
    ruby_boxes.push(RubyLayoutBox {
        run_index,
        span_index,
        base_range: ruby.base_range.clone(),
        line: line_cursor + (line - line_cursor),
        x: ruby_x,
        y: ruby_y,
        width: ruby_width,
        height: ruby_height,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_raw_layout(
    raw: RawLayout,
    state: &mut FontState,
    run_index: usize,
    role: GlyphRole,
    line_offset: u32,
    y_offset: f32,
    source_text: &str,
    requested_direction: TextDirection,
    family_chain: &[String],
    lines: &mut Vec<LayoutLine>,
    shaped_runs: &mut Vec<ShapedGlyphRun>,
    resources: &mut BTreeMap<String, GlyphResource>,
    diagnostics: &mut Vec<Diagnostic>,
    config: &TextLayoutConfig,
) -> Result<(), MediaError> {
    let RawLayout {
        lines: raw_lines,
        locale,
        ..
    } = raw;
    for (line_index, raw_line) in raw_lines.into_iter().enumerate() {
        validate_direction(requested_direction, raw_line.rtl)?;
        let line_number = line_offset + line_index as u32;
        lines.push(LayoutLine {
            run_index,
            role: role.clone(),
            line: line_number,
            source: raw_line.source.clone(),
            rtl: raw_line.rtl,
            top: y_offset + raw_line.top,
            baseline: y_offset + raw_line.baseline,
            width: raw_line.width,
            height: raw_line.height,
        });
        for mut glyph in raw_line.glyphs {
            glyph.start += raw_line.source_offset;
            glyph.end += raw_line.source_offset;
            if glyph.start > glyph.end || glyph.end > source_text.len() {
                return Err(MediaError::message(
                    "ASTRA_TEXT_CLUSTER_RANGE: shaper returned an invalid source cluster",
                ));
            }
            let face = state
                .faces
                .get(&glyph.font_id.to_string())
                .cloned()
                .ok_or_else(|| {
                    MediaError::message(
                        "ASTRA_TEXT_UNPACKAGED_FACE: shaper selected a face outside the package database",
                    )
                })?;
            let family_index = family_chain
                .iter()
                .position(|family| family.eq_ignore_ascii_case(&face.family))
                .ok_or_else(|| {
                    MediaError::message(
                        "ASTRA_TEXT_FALLBACK_UNDECLARED: shaper selected an undeclared fallback face",
                    )
                })?;
            if glyph.glyph_id == 0 {
                return Err(MediaError::message(
                    "ASTRA_TEXT_GLYPH_MISSING: packaged fallback chain does not cover a source cluster",
                ));
            }
            let cluster = &source_text[glyph.start..glyph.end];
            if !cluster_covered(cluster, &face.coverage) {
                return Err(MediaError::message(
                    "ASTRA_TEXT_FONT_COVERAGE: shaped cluster is outside the packaged coverage declaration",
                ));
            }
            let expected_family_index = family_chain
                .iter()
                .position(|family| {
                    state.faces.values().any(|candidate| {
                        candidate.family.eq_ignore_ascii_case(family)
                            && cluster_covered(cluster, &candidate.coverage)
                    })
                })
                .ok_or_else(|| {
                    MediaError::message(
                        "ASTRA_TEXT_GLYPH_MISSING: no declared fallback coverage contains the source cluster",
                    )
                })?;
            if family_index != expected_family_index {
                return Err(MediaError::message(
                    "ASTRA_TEXT_FALLBACK_ORDER: shaper did not select the first eligible declared fallback",
                ));
            }
            if family_index > 0
                && !diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "ASTRA_TEXT_FONT_FALLBACK"
                        && diagnostic
                            .fields
                            .get("family")
                            .is_some_and(|family| family == &face.family)
                })
            {
                tracing::debug!(
                    target: "astra_media::text",
                    event = "text.font.fallback",
                    family = %face.family,
                    asset_id = %face.asset_id,
                    fallback_index = family_index,
                );
                diagnostics.push(
                    Diagnostic::warning(
                        "ASTRA_TEXT_FONT_FALLBACK",
                        "shaping consumed an explicitly declared packaged fallback face",
                    )
                    .with_field("family", &face.family)
                    .with_field("asset_id", &face.asset_id),
                );
            }
            let direction = if glyph.level.number() % 2 == 0 {
                TextDirection::LeftToRight
            } else {
                TextDirection::RightToLeft
            };
            let physical = glyph.physical((0.0, y_offset + raw_line.baseline), 1.0);
            let image = {
                let system = state.font_systems.get_mut(&locale).ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_FONT_SYSTEM: locale system is missing")
                })?;
                state
                    .swash_cache
                    .get_image(system, physical.cache_key)
                    .clone()
            };
            let invisible = cluster
                .chars()
                .all(|value| value.is_whitespace() || is_invisible_scalar(value));
            let empty_image = image.as_ref().is_some_and(|image| {
                image.placement.width == 0 || image.placement.height == 0 || image.data.is_empty()
            });
            let zero_advance = glyph.w.abs() <= f32::EPSILON;
            let (resource_id, render_x, render_y) = if empty_image && (invisible || zero_advance) {
                (None, None, None)
            } else if let Some(image) = image {
                let bitmap = glyph_bitmap(&image)?;
                let mut identity = Vec::new();
                identity.extend_from_slice(face.hash.as_bytes());
                identity.extend_from_slice(&glyph.glyph_id.to_le_bytes());
                identity.extend_from_slice(bitmap.hash.as_bytes());
                let resource_id = format!("glyph:{}", Hash256::from_sha256(&identity).to_hex());
                resources
                    .entry(resource_id.clone())
                    .or_insert_with(|| GlyphResource {
                        resource_id: resource_id.clone(),
                        font_asset_id: face.asset_id.clone(),
                        font_hash: face.hash,
                        glyph_id: glyph.glyph_id,
                        bitmap,
                    });
                (
                    Some(resource_id),
                    Some(physical.x + image.placement.left),
                    Some(physical.y - image.placement.top),
                )
            } else if invisible {
                (None, None, None)
            } else {
                return Err(MediaError::message(
                    "ASTRA_TEXT_GLYPH_RASTER: covered glyph could not be rasterized",
                ));
            };
            let shaped = ShapedGlyph {
                source: SourceRange {
                    start: glyph.start,
                    end: glyph.end,
                },
                glyph_id: glyph.glyph_id,
                font_asset_id: face.asset_id.clone(),
                font_family: face.family.clone(),
                font_face_index: face.face_index,
                font_hash: face.hash,
                direction,
                x: glyph.x,
                y: y_offset + raw_line.top + glyph.y,
                advance: glyph.w,
                baseline: y_offset + raw_line.baseline,
                line: line_number,
                resource_id,
                render_x,
                render_y,
                rotation_quadrants: 0,
                tate_chu_yoko: false,
            };
            append_shaped_glyph(shaped_runs, run_index, role.clone(), shaped);
        }
        if shaped_runs
            .iter()
            .map(|run| run.glyphs.len())
            .sum::<usize>()
            > config.max_glyphs
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_GLYPH_BUDGET: shaped glyph count exceeds the configured budget",
            ));
        }
    }
    Ok(())
}

fn is_vertical(direction: TextDirection) -> bool {
    matches!(
        direction,
        TextDirection::VerticalRightToLeft | TextDirection::VerticalLeftToRight
    )
}

fn shaping_constraint(constraint: LayoutConstraint, direction: TextDirection) -> LayoutConstraint {
    if !is_vertical(direction) {
        return constraint;
    }
    LayoutConstraint {
        max_width: constraint.max_height.unwrap_or(constraint.max_width),
        max_height: Some(constraint.max_width),
        ..constraint
    }
}

fn transform_vertical_run(
    run: &TextRun,
    run_index: usize,
    line_cursor: u32,
    constraint: LayoutConstraint,
    lines: &mut [LayoutLine],
    shaped_runs: &mut [ShapedGlyphRun],
    ruby_boxes: &mut [RubyLayoutBox],
) -> Result<(), MediaError> {
    let column_width = constraint.line_height;
    let max_width = constraint.max_width;
    let digit_groups = vertical_digit_groups(&run.text);
    for line in lines.iter_mut().filter(|line| line.run_index == run_index) {
        let column = line.line.saturating_sub(line_cursor) as f32;
        let mut x = match run.direction {
            TextDirection::VerticalRightToLeft => max_width - (column + 1.0) * column_width,
            TextDirection::VerticalLeftToRight => column * column_width,
            _ => unreachable!("vertical transform is gated by direction"),
        };
        if matches!(line.role, GlyphRole::Ruby { .. }) {
            x += match run.direction {
                TextDirection::VerticalRightToLeft => column_width * 0.58,
                TextDirection::VerticalLeftToRight => -column_width * 0.58,
                _ => 0.0,
            };
        }
        let old_width = line.width;
        line.top = 0.0;
        line.baseline = x + constraint.font_size;
        line.width = column_width;
        line.height = old_width.max(column_width);
    }
    for shaped in shaped_runs
        .iter_mut()
        .filter(|shaped| shaped.run_index == run_index)
    {
        shaped.direction = run.direction;
        let column = shaped.line.saturating_sub(line_cursor) as f32;
        let base_x = match run.direction {
            TextDirection::VerticalRightToLeft => max_width - (column + 1.0) * column_width,
            TextDirection::VerticalLeftToRight => column * column_width,
            _ => unreachable!("vertical transform is gated by direction"),
        };
        let ruby_offset = if matches!(shaped.role, GlyphRole::Ruby { .. }) {
            match run.direction {
                TextDirection::VerticalRightToLeft => column_width * 0.58,
                TextDirection::VerticalLeftToRight => -column_width * 0.58,
                _ => 0.0,
            }
        } else {
            0.0
        };
        shaped.baseline = base_x + ruby_offset + constraint.font_size;
        let mut digit_group_origins = BTreeMap::<usize, f32>::new();
        for glyph in &shaped.glyphs {
            if let Some(group) = digit_groups
                .iter()
                .find(|range| glyph.source.start >= range.start && glyph.source.end <= range.end)
            {
                digit_group_origins
                    .entry(group.start)
                    .and_modify(|origin| *origin = origin.min(glyph.x))
                    .or_insert(glyph.x);
            }
        }
        for glyph in &mut shaped.glyphs {
            let old_x = glyph.x;
            let source = run
                .text
                .get(glyph.source.start..glyph.source.end)
                .unwrap_or("");
            let digit_group = digit_groups
                .iter()
                .find(|range| glyph.source.start >= range.start && glyph.source.end <= range.end);
            glyph.direction = run.direction;
            glyph.x = base_x + ruby_offset;
            glyph.y = old_x;
            glyph.baseline = shaped.baseline;
            glyph.rotation_quadrants =
                u8::from(digit_group.is_none() && source.chars().any(vertical_glyph_rotates));
            glyph.tate_chu_yoko = digit_group.is_some();
            if let Some(group) = digit_group {
                let group_text = &run.text[group.clone()];
                let scalar_count = group_text.chars().count().max(1) as f32;
                let scalar_index = run.text[group.start..glyph.source.start].chars().count() as f32;
                glyph.x = base_x + ruby_offset + scalar_index * constraint.font_size / scalar_count;
                glyph.y = *digit_group_origins.get(&group.start).ok_or_else(|| {
                    MediaError::message(
                        "ASTRA_TEXT_VERTICAL_TATE_CHU_YOKO: digit group has no shaped origin",
                    )
                })?;
                glyph.advance = constraint.font_size / scalar_count;
            } else {
                glyph.advance = column_width;
            }
            glyph.render_x = Some(glyph.x.round() as i32);
            glyph.render_y = Some(glyph.y.round() as i32);
        }
    }
    for ruby in ruby_boxes
        .iter_mut()
        .filter(|ruby| ruby.run_index == run_index)
    {
        let column = ruby.line.saturating_sub(line_cursor) as f32;
        ruby.x = match run.direction {
            TextDirection::VerticalRightToLeft => {
                max_width - (column + 1.0) * column_width + column_width * 0.58
            }
            TextDirection::VerticalLeftToRight => column * column_width - column_width * 0.58,
            _ => unreachable!("vertical transform is gated by direction"),
        };
        std::mem::swap(&mut ruby.width, &mut ruby.height);
    }
    Ok(())
}

fn vertical_digit_groups(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut groups = Vec::new();
    let mut start = None;
    let mut count = 0usize;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if character.is_ascii_digit() {
            start.get_or_insert(index);
            count += 1;
        } else if let Some(group_start) = start.take() {
            if (2..=4).contains(&count) {
                groups.push(group_start..index);
            }
            count = 0;
        }
    }
    groups
}

fn vertical_glyph_rotates(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '!'..='/' | ':'..='@' | '['..='`' | '{'..='~')
}

fn is_invisible_scalar(value: char) -> bool {
    value.is_control()
        || matches!(
            value as u32,
            0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f
        )
}
