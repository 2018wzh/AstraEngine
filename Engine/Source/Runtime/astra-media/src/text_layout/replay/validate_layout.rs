use super::*;

pub(super) fn validate_layout(
    layout: &TextLayoutResult,
    limits: TextLayoutReplayLimits,
) -> Result<(), MediaError> {
    if layout.schema != TEXT_LAYOUT_SCHEMA
        || layout.key.trim().is_empty()
        || !layout.width.is_finite()
        || !layout.height.is_finite()
        || layout.width < 0.0
        || layout.height < 0.0
    {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_LAYOUT",
            "recorded shaped layout schema or key is invalid",
        ));
    }
    if layout.glyph_resources.len() > limits.max_glyph_resources_per_record {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_GLYPH_BUDGET",
            "recorded shaped layout exceeds the glyph resource budget",
        ));
    }
    let mut resource_ids = BTreeSet::new();
    let mut glyph_bytes = 0usize;
    for resource in &layout.glyph_resources {
        let channels = match resource.bitmap.format {
            GlyphBitmapFormat::Alpha8 => 1usize,
            GlyphBitmapFormat::Rgba8 => 4usize,
        };
        let expected_len = (resource.bitmap.width as usize)
            .checked_mul(resource.bitmap.height as usize)
            .and_then(|pixels| pixels.checked_mul(channels))
            .ok_or_else(|| {
                text_replay_error(
                    "ASTRA_TEXT_REPLAY_GLYPH_BOUNDS",
                    "recorded glyph bitmap dimensions overflow",
                )
            })?;
        if resource.bitmap.width == 0
            || resource.bitmap.height == 0
            || !safe_resource_id(&resource.resource_id)
            || !resource_ids.insert(resource.resource_id.clone())
            || expected_len != resource.bitmap.pixels.len()
        {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_GLYPH",
                "recorded glyph resource identity or dimensions are invalid",
            ));
        }
        glyph_bytes = glyph_bytes.checked_add(expected_len).ok_or_else(|| {
            text_replay_error(
                "ASTRA_TEXT_REPLAY_GLYPH_BOUNDS",
                "recorded glyph byte count overflowed",
            )
        })?;
    }
    if glyph_bytes > limits.max_glyph_bytes_per_record {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_GLYPH_BUDGET",
            "recorded shaped layout exceeds the glyph byte budget",
        ));
    }
    for run in &layout.shaped_runs {
        for glyph in &run.glyphs {
            if glyph
                .resource_id
                .as_ref()
                .is_some_and(|resource_id| !resource_ids.contains(resource_id))
            {
                return Err(text_replay_error(
                    "ASTRA_TEXT_REPLAY_GLYPH_REFERENCE",
                    "recorded shaped glyph references an undeclared bitmap",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn ensure_snapshot_budget(
    snapshot: &TextLayoutReplaySnapshot,
) -> Result<(), MediaError> {
    let bytes = postcard::to_allocvec(snapshot).map_err(|error| {
        text_replay_error(
            "ASTRA_TEXT_REPLAY_SERIALIZATION",
            format!("layout replay snapshot serialization failed: {error}"),
        )
    })?;
    if bytes.len() > snapshot.limits.max_snapshot_bytes {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_SNAPSHOT_BUDGET",
            "layout replay snapshot exceeds the configured byte budget",
        ));
    }
    Ok(())
}

pub(super) fn binding_hash(binding: &TextLayoutBindingIdentity) -> Result<Hash256, MediaError> {
    canonical_hash(&(TEXT_LAYOUT_REPLAY_SCHEMA, binding))
}

pub(super) fn record_hash(record: &TextLayoutReplayRecord) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        &record.schema,
        record.sequence,
        record.request_hash,
        record.binding_hash,
        PersistedLayoutRef(&record.layout),
    ))
}

pub(super) struct PersistedLayoutRef<'a>(pub(super) &'a TextLayoutResult);

impl Serialize for PersistedLayoutRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        binary_text_layout::serialize(self.0, serializer)
    }
}

pub(super) fn transcript_hash(snapshot: &TextLayoutReplaySnapshot) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        &snapshot.schema,
        &snapshot.binding,
        snapshot.limits,
        snapshot.next_sequence,
        &snapshot.records,
    ))
}

pub(super) fn snapshot_hash(snapshot: &TextLayoutReplaySnapshot) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA,
        snapshot.transcript_hash,
        snapshot.replay_cursor,
    ))
}

pub(super) fn canonical_hash<T: Serialize>(value: &T) -> Result<Hash256, MediaError> {
    let bytes = postcard::to_allocvec(value).map_err(|error| {
        text_replay_error(
            "ASTRA_TEXT_REPLAY_SERIALIZATION",
            format!("layout replay identity serialization failed: {error}"),
        )
    })?;
    Ok(Hash256::from_sha256(&bytes))
}

pub(super) fn safe_resource_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-')
        })
}

pub(super) fn safe_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(super) fn valid_coverage(ranges: &[super::super::UnicodeRange]) -> bool {
    let mut previous_end = None;
    ranges.iter().all(|range| {
        let valid = range.start <= range.end
            && range.end <= char::MAX as u32
            && previous_end.is_none_or(|end| range.start > end);
        previous_end = Some(range.end);
        valid
    })
}

pub(super) fn text_replay_error(code: &str, message: impl AsRef<str>) -> MediaError {
    MediaError::message(format!("{code}: {}", message.as_ref()))
}
