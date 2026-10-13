use super::*;

pub const TEXT_LAYOUT_REPLAY_SCHEMA: &str = "astra.text_layout_replay.v1";

pub const TEXT_LAYOUT_REPLAY_SNAPSHOT_SCHEMA: &str = "astra.text_layout_replay_snapshot.v1";

/// Computes the persisted replay identity for a validated request.
///
/// This function belongs to the replay/save domain and must not be called by
/// ordinary layout, measurement, rendering, or cache lookup paths.
pub fn text_layout_replay_request_hash(request: &TextLayoutRequest) -> Result<Hash256, MediaError> {
    canonical_hash(&(TEXT_LAYOUT_SCHEMA, request))
}

/// Computes a persisted/Evidence digest without adding identity work to the
/// live layout result.
pub fn text_layout_evidence_hash(layout: &TextLayoutResult) -> Result<Hash256, MediaError> {
    canonical_hash(&PersistedLayoutRef(layout))
}

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
    #[schemars(with = "binary_text_layout::LayoutWire")]
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
pub(super) enum SessionMode {
    Live,
    Replay,
}

/// Bounded, provider-bound layout transcript used by save/load and provider-free replay.
///
/// Records intentionally omit source text. They retain the immutable shaped result and glyph
/// resources needed to reproduce presentation without loading a live font provider.
pub struct TextLayoutReplaySession {
    pub(super) snapshot: TextLayoutReplaySnapshot,
    pub(super) mode: SessionMode,
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
        // Replay identity is persisted-domain work. The ordinary layout/cache
        // path never serializes or hashes the request.
        let request_hash = text_layout_replay_request_hash(request)?;
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
            layout_revision = layout.revision,
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
            layout_revision = layout.revision,
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

    pub(super) fn restore(
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

pub(super) fn validate_limits(limits: TextLayoutReplayLimits) -> Result<(), MediaError> {
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

pub(super) fn validate_binding(binding: &TextLayoutBindingIdentity) -> Result<(), MediaError> {
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

pub(super) fn validate_snapshot(snapshot: &TextLayoutReplaySnapshot) -> Result<(), MediaError> {
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
