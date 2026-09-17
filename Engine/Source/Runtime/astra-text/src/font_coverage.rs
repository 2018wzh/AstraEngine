use crate::{MediaError, UnicodeRange};
use cosmic_text::skrifa::prelude::*;
use std::collections::BTreeSet;

/// Read the selected face's character map into ordered, disjoint scalar ranges.
/// This describes actual font mappings; it does not replace shaping or fallback checks.
pub fn font_unicode_coverage(
    bytes: &[u8],
    face_index: u32,
) -> Result<Vec<UnicodeRange>, MediaError> {
    let font = FontRef::from_index(bytes, face_index)
        .map_err(|_| MediaError::message("ASTRA_TEXT_FONT_PARSE: font face cannot be read"))?;
    let codepoints = font
        .charmap()
        .mappings()
        .filter_map(|(codepoint, glyph)| {
            (glyph.to_u32() != 0 && char::from_u32(codepoint).is_some()).then_some(codepoint)
        })
        .collect::<BTreeSet<_>>();
    let mut ranges: Vec<UnicodeRange> = Vec::new();
    for codepoint in codepoints {
        if let Some(last) = ranges.last_mut() {
            if last.end + 1 == codepoint {
                last.end = codepoint;
                continue;
            }
        }
        ranges.push(UnicodeRange {
            start: codepoint,
            end: codepoint,
        });
    }
    if ranges.is_empty() {
        return Err(MediaError::message(
            "ASTRA_TEXT_FONT_COVERAGE: font face has no Unicode mappings",
        ));
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_font_data_is_rejected() {
        assert!(font_unicode_coverage(b"invalid", 0).is_err());
    }
}
