use crate::{
    GlyphRole, LayoutConstraint, LayoutLine, MediaError, RubyLayoutBox, ShapedGlyphRun,
    TextDirection, TextRun,
};
use std::collections::BTreeMap;

pub(super) fn is_vertical(direction: TextDirection) -> bool {
    matches!(
        direction,
        TextDirection::VerticalRightToLeft | TextDirection::VerticalLeftToRight
    )
}

pub(super) fn shaping_constraint(
    constraint: LayoutConstraint,
    direction: TextDirection,
) -> LayoutConstraint {
    if !is_vertical(direction) {
        return constraint;
    }
    LayoutConstraint {
        max_width: constraint.max_height.unwrap_or(constraint.max_width),
        max_height: Some(constraint.max_width),
        ..constraint
    }
}

pub(super) fn transform_vertical_run(
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
