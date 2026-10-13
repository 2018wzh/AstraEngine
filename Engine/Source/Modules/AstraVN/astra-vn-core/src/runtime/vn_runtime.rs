use super::*;

#[derive(Debug, Clone)]
pub struct VnRuntime {
    pub(super) compiled: Arc<CompiledStory>,
    pub(super) index: Arc<VnRuntimeIndex>,
    pub(super) state: VnRuntimeState,
    pub(super) mutation_journal: BTreeMap<(String, String), Option<i64>>,
}

#[derive(Debug)]
pub struct VnRuntimeIndex {
    pub(super) story_hash: Hash128,
    pub(super) state_ids: BTreeSet<String>,
    pub(super) state_ordinals: BTreeMap<String, u32>,
    pub(super) states_by_ordinal: Vec<String>,
    pub(super) command_ordinals: BTreeMap<String, u32>,
    pub(super) commands_by_ordinal: Vec<String>,
    pub(super) commands_by_state: BTreeMap<String, Vec<(usize, usize)>>,
    pub(super) story_by_state: BTreeMap<String, String>,
}

impl VnRuntimeIndex {
    pub fn build(compiled: &CompiledStory) -> Result<Self, VnError> {
        let mut story_by_state = BTreeMap::new();
        for story in &compiled.stories {
            for state_id in &story.states {
                if !compiled.states.contains_key(state_id) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_RUNTIME_INDEX_STATE_MISSING",
                        format!("story {} references missing state {state_id}", story.id),
                    ));
                }
                if story_by_state
                    .insert(state_id.clone(), story.id.clone())
                    .is_some()
                {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_RUNTIME_INDEX_STATE_OWNER_CONFLICT",
                        format!("state {state_id} belongs to multiple stories"),
                    ));
                }
            }
        }
        let commands_by_state: BTreeMap<String, Vec<(usize, usize)>> = compiled
            .states
            .iter()
            .map(|(state_id, state)| {
                let commands = state
                    .scenes
                    .iter()
                    .enumerate()
                    .flat_map(|(scene_index, scene)| {
                        (0..scene.commands.len())
                            .map(move |command_index| (scene_index, command_index))
                    })
                    .collect();
                (state_id.clone(), commands)
            })
            .collect();
        let states_by_ordinal = compiled.states.keys().cloned().collect::<Vec<_>>();
        let state_ordinals = states_by_ordinal
            .iter()
            .enumerate()
            .map(|(ordinal, id)| (id.clone(), ordinal as u32))
            .collect();
        let commands_by_ordinal = compiled
            .states
            .values()
            .flat_map(|state| state.scenes.iter())
            .flat_map(|scene| scene.commands.iter())
            .map(compiled_command_id)
            .collect::<Vec<_>>();
        let command_ordinals = commands_by_ordinal
            .iter()
            .enumerate()
            .map(|(ordinal, id)| (id.clone(), ordinal as u32))
            .collect();
        Ok(Self {
            story_hash: compiled.story_hash,
            state_ids: compiled.states.keys().cloned().collect(),
            state_ordinals,
            states_by_ordinal,
            command_ordinals,
            commands_by_ordinal,
            commands_by_state,
            story_by_state,
        })
    }

    pub(super) fn validate_story(&self, compiled: &CompiledStory) -> Result<(), VnError> {
        if self.story_hash != compiled.story_hash {
            return Err(VnError::diagnostic(
                "ASTRA_VN_RUNTIME_INDEX_STORY_MISMATCH",
                "runtime index does not belong to the compiled story",
            ));
        }
        Ok(())
    }

    pub fn encode_read_state(&self, values: &BTreeSet<String>) -> Result<Vec<u64>, VnError> {
        encode_ordinals(
            values,
            &self.command_ordinals,
            "ASTRA_VN_READ_STATE_COMMAND_UNKNOWN",
        )
    }

    pub fn decode_read_state(&self, bits: &[u64]) -> Result<BTreeSet<String>, VnError> {
        decode_ordinals(
            bits,
            &self.commands_by_ordinal,
            "ASTRA_VN_READ_STATE_BITS_INVALID",
        )
    }

    pub fn encode_route_coverage(&self, values: &BTreeSet<String>) -> Result<Vec<u64>, VnError> {
        encode_ordinals(
            values,
            &self.state_ordinals,
            "ASTRA_VN_ROUTE_COVERAGE_STATE_UNKNOWN",
        )
    }

    pub fn decode_route_coverage(&self, bits: &[u64]) -> Result<BTreeSet<String>, VnError> {
        decode_ordinals(
            bits,
            &self.states_by_ordinal,
            "ASTRA_VN_ROUTE_COVERAGE_BITS_INVALID",
        )
    }
}

pub(super) fn encode_ordinals(
    values: &BTreeSet<String>,
    ordinals: &BTreeMap<String, u32>,
    diagnostic: &'static str,
) -> Result<Vec<u64>, VnError> {
    let mut bits = vec![0_u64; ordinals.len().div_ceil(64)];
    for value in values {
        let ordinal = *ordinals.get(value).ok_or_else(|| {
            VnError::diagnostic(
                diagnostic,
                "runtime history references an unknown stable id",
            )
        })? as usize;
        bits[ordinal / 64] |= 1_u64 << (ordinal % 64);
    }
    Ok(bits)
}

pub(super) fn decode_ordinals(
    bits: &[u64],
    values: &[String],
    diagnostic: &'static str,
) -> Result<BTreeSet<String>, VnError> {
    let expected_words = values.len().div_ceil(64);
    if bits.len() != expected_words {
        return Err(VnError::diagnostic(
            diagnostic,
            "runtime history bitset has an invalid word count",
        ));
    }
    if let Some(last) = bits.last() {
        let used = values.len() % 64;
        if used != 0 && last >> used != 0 {
            return Err(VnError::diagnostic(
                diagnostic,
                "runtime history bitset contains out-of-range ordinals",
            ));
        }
    }
    Ok(values
        .iter()
        .enumerate()
        .filter(|(ordinal, _)| bits[ordinal / 64] & (1_u64 << (ordinal % 64)) != 0)
        .map(|(_, value)| value.clone())
        .collect())
}
