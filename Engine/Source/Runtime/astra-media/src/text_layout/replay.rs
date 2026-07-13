use std::collections::BTreeSet;

use astra_core::Hash256;
use astra_media_core::GlyphBitmapFormat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

use super::{
    TextLayoutProvider, TextLayoutProviderIdentity, TextLayoutRequest, TextLayoutResult,
    TEXT_LAYOUT_SCHEMA,
};

pub const TEXT_LAYOUT_REPLAY_SCHEMA: &str = "astra.text_layout_replay.v1";
pub const TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA: &str = "astra.text_layout_replay_snapshot.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutReplayLimits {
    pub max_records: usize,
    pub max_snapshot_bytes: usize,
    pub max_glyph_resources_per_record: usize,
    pub max_glyph_bytes_per_record: usize,
}

impl TextLayoutReplayLimits {
    pub const fn production_defaults() -> Self {
        Self {
            max_records: 16_384,
            max_snapshot_bytes: 256 * 1024 * 1024,
            max_glyph_resources_per_record: 65_536,
            max_glyph_bytes_per_record: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutBindingIdentity {
    pub provider_id: String,
    pub provider_fingerprint: Hash256,
    pub package_hash: Hash256,
    pub build_fingerprint: Hash256,
    pub session_id: String,
    pub provider: TextLayoutProviderIdentity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutReplayRecord {
    pub schema: String,
    pub sequence: u64,
    pub request_hash: Hash256,
    pub binding_hash: Hash256,
    #[serde(with = "binary_text_layout")]
    #[schemars(with = "TextLayoutResult")]
    pub layout: TextLayoutResult,
    pub record_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutReplayInput {
    pub sequence: u64,
    pub request_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLayoutReplaySnapshot {
    pub schema: String,
    pub binding: TextLayoutBindingIdentity,
    pub limits: TextLayoutReplayLimits,
    pub next_sequence: u64,
    pub replay_cursor: usize,
    pub records: Vec<TextLayoutReplayRecord>,
    pub transcript_hash: Hash256,
    pub snapshot_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionMode {
    Live,
    Replay,
}

/// Bounded, provider-bound layout transcript used by save/load and provider-free replay.
///
/// Records intentionally omit source text. They retain the immutable shaped result and glyph
/// resources needed to reproduce presentation without loading a live font provider.
pub struct TextLayoutReplaySession {
    snapshot: TextLayoutReplaySnapshot,
    mode: SessionMode,
}

impl TextLayoutReplaySession {
    pub fn live(
        binding: TextLayoutBindingIdentity,
        limits: TextLayoutReplayLimits,
    ) -> Result<Self, MediaError> {
        validate_limits(limits)?;
        validate_binding(&binding)?;
        let mut snapshot = TextLayoutReplaySnapshot {
            schema: TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA.to_string(),
            binding,
            limits,
            next_sequence: 1,
            replay_cursor: 0,
            records: Vec::new(),
            transcript_hash: Hash256::from_sha256(&[]),
            snapshot_hash: Hash256::from_sha256(&[]),
        };
        snapshot.transcript_hash = transcript_hash(&snapshot)?;
        snapshot.snapshot_hash = snapshot_hash(&snapshot)?;
        Ok(Self {
            snapshot,
            mode: SessionMode::Live,
        })
    }

    pub fn record_live<P: TextLayoutProvider>(
        &mut self,
        provider: &P,
        request: &TextLayoutRequest,
    ) -> Result<TextLayoutResult, MediaError> {
        if self.mode != SessionMode::Live {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_MODE",
                "provider layout cannot execute in provider-free replay mode",
            ));
        }
        if self.snapshot.records.len() >= self.snapshot.limits.max_records {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_RECORD_BUDGET",
                "layout record count exceeds the configured snapshot budget",
            ));
        }
        let actual_identity = provider.identity()?;
        if actual_identity != self.snapshot.binding.provider {
            return Err(text_replay_error(
                "ASTRA_TEXT_PROVIDER_DRIFT",
                "live text provider or packaged font identity changed",
            ));
        }
        let request_hash = provider.request_hash(request)?;
        let layout = provider.layout(request)?;
        validate_layout(&layout, self.snapshot.limits)?;
        let binding_hash = binding_hash(&self.snapshot.binding)?;
        let sequence = self.snapshot.next_sequence;
        let mut record = TextLayoutReplayRecord {
            schema: TEXT_LAYOUT_REPLAY_SCHEMA.to_string(),
            sequence,
            request_hash,
            binding_hash,
            layout: layout.clone(),
            record_hash: Hash256::from_sha256(&[]),
        };
        record.record_hash = record_hash(&record)?;

        let mut next = self.snapshot.clone();
        next.next_sequence = next.next_sequence.checked_add(1).ok_or_else(|| {
            text_replay_error(
                "ASTRA_TEXT_REPLAY_SEQUENCE",
                "layout replay sequence overflowed",
            )
        })?;
        next.records.push(record);
        next.transcript_hash = transcript_hash(&next)?;
        next.snapshot_hash = snapshot_hash(&next)?;
        ensure_snapshot_budget(&next)?;
        self.snapshot = next;
        tracing::debug!(
            target: "astra_media::text",
            event = "text.layout.recorded",
            sequence,
            request_hash = %request_hash,
            layout_hash = %layout.hash,
            record_count = self.snapshot.records.len(),
        );
        Ok(layout)
    }

    pub fn replay_next(
        &mut self,
        input: TextLayoutReplayInput,
    ) -> Result<TextLayoutResult, MediaError> {
        if self.mode != SessionMode::Replay {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_MODE",
                "recorded layout can only be consumed in provider-free replay mode",
            ));
        }
        let record = self
            .snapshot
            .records
            .get(self.snapshot.replay_cursor)
            .ok_or_else(|| {
                text_replay_error(
                    "ASTRA_TEXT_REPLAY_EXHAUSTED",
                    "provider-free layout transcript has no next record",
                )
            })?;
        if record.sequence != input.sequence || record.request_hash != input.request_hash {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_REQUEST_DRIFT",
                "replay sequence or request identity does not match the recorded layout",
            ));
        }
        let sequence = record.sequence;
        let layout = record.layout.clone();
        let next_cursor = self.snapshot.replay_cursor.checked_add(1).ok_or_else(|| {
            text_replay_error(
                "ASTRA_TEXT_REPLAY_SEQUENCE",
                "layout replay cursor overflowed",
            )
        })?;
        let mut next = self.snapshot.clone();
        next.replay_cursor = next_cursor;
        next.snapshot_hash = snapshot_hash(&next)?;
        self.snapshot = next;
        tracing::trace!(
            target: "astra_media::text",
            event = "text.layout.replayed",
            sequence,
            request_hash = %input.request_hash,
            layout_hash = %layout.hash,
        );
        Ok(layout)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, MediaError> {
        validate_snapshot(&self.snapshot)?;
        let bytes = postcard::to_allocvec(&self.snapshot).map_err(|error| {
            text_replay_error(
                "ASTRA_TEXT_REPLAY_SERIALIZATION",
                format!("layout replay snapshot serialization failed: {error}"),
            )
        })?;
        if bytes.len() > self.snapshot.limits.max_snapshot_bytes {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_SNAPSHOT_BUDGET",
                "layout replay snapshot exceeds the configured byte budget",
            ));
        }
        Ok(bytes)
    }

    pub fn restore_live(
        bytes: &[u8],
        expected_binding: &TextLayoutBindingIdentity,
        maximum_snapshot_bytes: usize,
    ) -> Result<Self, MediaError> {
        Self::restore(
            bytes,
            expected_binding,
            maximum_snapshot_bytes,
            SessionMode::Live,
        )
    }

    pub fn restore_replay(
        bytes: &[u8],
        expected_binding: &TextLayoutBindingIdentity,
        maximum_snapshot_bytes: usize,
    ) -> Result<Self, MediaError> {
        Self::restore(
            bytes,
            expected_binding,
            maximum_snapshot_bytes,
            SessionMode::Replay,
        )
    }

    pub fn transcript_hash(&self) -> Hash256 {
        self.snapshot.transcript_hash
    }

    pub fn recorded_layouts(&self) -> usize {
        self.snapshot.records.len()
    }

    pub fn replayed_layouts(&self) -> usize {
        self.snapshot.replay_cursor
    }

    fn restore(
        bytes: &[u8],
        expected_binding: &TextLayoutBindingIdentity,
        maximum_snapshot_bytes: usize,
        mode: SessionMode,
    ) -> Result<Self, MediaError> {
        if maximum_snapshot_bytes == 0 || bytes.len() > maximum_snapshot_bytes {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_SNAPSHOT_BUDGET",
                "layout replay snapshot exceeds the host restore budget",
            ));
        }
        let snapshot: TextLayoutReplaySnapshot = postcard::from_bytes(bytes).map_err(|error| {
            text_replay_error(
                "ASTRA_TEXT_REPLAY_SNAPSHOT",
                format!("layout replay snapshot decode failed: {error}"),
            )
        })?;
        validate_snapshot(&snapshot)?;
        if &snapshot.binding != expected_binding {
            return Err(text_replay_error(
                "ASTRA_TEXT_PROVIDER_DRIFT",
                "snapshot package, provider, target, profile, or font identity changed",
            ));
        }
        if bytes.len() > snapshot.limits.max_snapshot_bytes {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_SNAPSHOT_BUDGET",
                "layout replay snapshot exceeds its declared byte budget",
            ));
        }
        if mode == SessionMode::Live && snapshot.replay_cursor != 0 {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_MODE",
                "a consumed replay snapshot cannot resume as a live layout session",
            ));
        }
        Ok(Self { snapshot, mode })
    }
}

fn validate_limits(limits: TextLayoutReplayLimits) -> Result<(), MediaError> {
    if limits.max_records == 0
        || limits.max_snapshot_bytes == 0
        || limits.max_glyph_resources_per_record == 0
        || limits.max_glyph_bytes_per_record == 0
    {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_LIMITS",
            "layout replay limits must be non-zero",
        ));
    }
    Ok(())
}

fn validate_binding(binding: &TextLayoutBindingIdentity) -> Result<(), MediaError> {
    if !safe_symbol(&binding.provider_id)
        || !safe_symbol(&binding.session_id)
        || !safe_symbol(&binding.provider.context.target)
        || !safe_symbol(&binding.provider.context.profile)
        || !safe_language(&binding.provider.context.default_locale)
        || binding.provider.fonts.is_empty()
    {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_BINDING",
            "layout replay binding identity is incomplete or unsafe",
        ));
    }
    let mut asset_ids = BTreeSet::new();
    let mut faces = BTreeSet::new();
    let mut previous_asset_id: Option<&str> = None;
    for font in &binding.provider.fonts {
        if !safe_resource_id(&font.asset_id)
            || font.family.trim().is_empty()
            || font.family.len() > 128
            || !safe_symbol(&font.license_id)
            || font
                .subset
                .as_ref()
                .is_some_and(|value| !safe_symbol(value))
            || font.coverage.is_empty()
            || !valid_coverage(&font.coverage)
            || !asset_ids.insert(font.asset_id.clone())
            || !faces.insert((font.hash, font.face_index))
            || previous_asset_id.is_some_and(|previous| previous >= font.asset_id.as_str())
        {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_FONT_BINDING",
                "layout replay font identity is invalid or duplicated",
            ));
        }
        previous_asset_id = Some(&font.asset_id);
    }
    Ok(())
}

fn validate_snapshot(snapshot: &TextLayoutReplaySnapshot) -> Result<(), MediaError> {
    if snapshot.schema != TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_SNAPSHOT_SCHEMA",
            "layout replay snapshot schema is unsupported",
        ));
    }
    validate_limits(snapshot.limits)?;
    validate_binding(&snapshot.binding)?;
    if snapshot.records.len() > snapshot.limits.max_records
        || snapshot.replay_cursor > snapshot.records.len()
        || snapshot.next_sequence != snapshot.records.len() as u64 + 1
    {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_SEQUENCE",
            "layout replay record count and sequence are inconsistent",
        ));
    }
    let expected_binding_hash = binding_hash(&snapshot.binding)?;
    for (index, record) in snapshot.records.iter().enumerate() {
        if record.schema != TEXT_LAYOUT_REPLAY_SCHEMA
            || record.sequence != index as u64 + 1
            || record.binding_hash != expected_binding_hash
            || record.record_hash != record_hash(record)?
        {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_RECORD",
                "layout replay record schema, sequence, binding, or hash is invalid",
            ));
        }
        validate_layout(&record.layout, snapshot.limits)?;
    }
    if snapshot.transcript_hash != transcript_hash(snapshot)? {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_TRANSCRIPT_HASH",
            "layout replay transcript hash does not match its records",
        ));
    }
    if snapshot.snapshot_hash != snapshot_hash(snapshot)? {
        return Err(text_replay_error(
            "ASTRA_TEXT_REPLAY_SNAPSHOT_HASH",
            "layout replay snapshot hash does not match its cursor",
        ));
    }
    Ok(())
}

fn validate_layout(
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
            || Hash256::from_sha256(&resource.bitmap.pixels) != resource.bitmap.hash
        {
            return Err(text_replay_error(
                "ASTRA_TEXT_REPLAY_GLYPH",
                "recorded glyph resource identity, dimensions, or hash is invalid",
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

fn ensure_snapshot_budget(snapshot: &TextLayoutReplaySnapshot) -> Result<(), MediaError> {
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

fn binding_hash(binding: &TextLayoutBindingIdentity) -> Result<Hash256, MediaError> {
    canonical_hash(&(TEXT_LAYOUT_REPLAY_SCHEMA, binding))
}

fn record_hash(record: &TextLayoutReplayRecord) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        &record.schema,
        record.sequence,
        record.request_hash,
        record.binding_hash,
        &record.layout,
    ))
}

fn transcript_hash(snapshot: &TextLayoutReplaySnapshot) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        &snapshot.schema,
        &snapshot.binding,
        snapshot.limits,
        snapshot.next_sequence,
        &snapshot.records,
    ))
}

fn snapshot_hash(snapshot: &TextLayoutReplaySnapshot) -> Result<Hash256, MediaError> {
    canonical_hash(&(
        TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA,
        snapshot.transcript_hash,
        snapshot.replay_cursor,
    ))
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<Hash256, MediaError> {
    let bytes = postcard::to_allocvec(value).map_err(|error| {
        text_replay_error(
            "ASTRA_TEXT_REPLAY_SERIALIZATION",
            format!("layout replay identity serialization failed: {error}"),
        )
    })?;
    Ok(Hash256::from_sha256(&bytes))
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_resource_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-')
        })
}

fn safe_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_coverage(ranges: &[super::UnicodeRange]) -> bool {
    let mut previous_end = None;
    ranges.iter().all(|range| {
        let valid = range.start <= range.end
            && range.end <= char::MAX as u32
            && previous_end.is_none_or(|end| range.start > end);
        previous_end = Some(range.end);
        valid
    })
}

fn text_replay_error(code: &str, message: impl AsRef<str>) -> MediaError {
    MediaError::message(format!("{code}: {}", message.as_ref()))
}

mod binary_text_layout {
    use astra_core::{Diagnostic, DiagnosticSeverity, Hash256, SourceSpan};
    use astra_media_core::GlyphBitmap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::super::{
        GlyphResource, GlyphRole, LayoutClip, LayoutLine, RubyLayoutBox, ShapedGlyph,
        ShapedGlyphRun, SourceRange, TextDirection, TextLayoutResult, VoiceReplayRefRecord,
    };

    #[derive(Serialize, Deserialize)]
    struct LayoutWire {
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
        hash: Hash256,
    }

    #[derive(Serialize, Deserialize)]
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

    #[derive(Serialize, Deserialize)]
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

    #[derive(Serialize, Deserialize)]
    enum RoleWire {
        Base,
        Ruby(usize),
    }

    #[derive(Serialize, Deserialize)]
    struct GlyphResourceWire {
        resource_id: String,
        font_asset_id: String,
        font_hash: Hash256,
        glyph_id: u16,
        bitmap: GlyphBitmap,
    }

    #[derive(Serialize, Deserialize)]
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
                hash: layout.hash,
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
                hash: layout.hash,
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
                bitmap: resource.bitmap.clone(),
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
                bitmap: resource.bitmap,
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
