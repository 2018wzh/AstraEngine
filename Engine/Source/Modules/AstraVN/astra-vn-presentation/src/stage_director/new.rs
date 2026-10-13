use super::*;

impl ProductStageDirector {
    pub fn new(
        manifest: VnPresentationProviderManifest,
        profile: impl Into<String>,
        viewport: StageViewport,
    ) -> Result<Self, VnError> {
        let profile = profile.into();
        manifest.profile(&profile).map_err(VnError::Diagnostic)?;
        validate_viewport(viewport)?;
        Ok(Self {
            manifest,
            state: ProductStageState {
                schema: PRODUCT_STAGE_STATE_SCHEMA.to_string(),
                profile,
                configured: false,
                viewport,
                safe_area: AspectRatio {
                    width: 16,
                    height: 9,
                },
                preloaded_assets: BTreeSet::new(),
                layers: BTreeMap::new(),
                entities: BTreeMap::new(),
                camera: ProductStageCamera::default(),
                movies: BTreeMap::new(),
                effects: BTreeMap::new(),
                backdrop_color: None,
                shade_color: [0, 0, 0, 255],
                shade_opacity: FixedScalar::ZERO,
                skip_allowed: true,
                transition: None,
                audio_bus_enabled: BTreeMap::from([
                    (VnAudioBus::Bgm, true),
                    (VnAudioBus::Se, true),
                ]),
                frame_index: 0,
                elapsed_ns: 0,
            },
            tweens: Vec::new(),
            timelines: BTreeMap::new(),
            settled_timelines: BTreeSet::new(),
            shake: None,
            coordinator: PresentationCoordinator::default(),
            queued_region_commands: BTreeMap::new(),
            queued_stage_commands: VecDeque::new(),
            next_region_sequence: 1,
            failed: false,
        })
    }

    pub fn is_failed(&self) -> bool {
        self.failed || self.coordinator.is_failed()
    }

    pub(super) fn ensure_active(&self) -> Result<(), VnError> {
        if self.is_failed() {
            return Err(stage_error(
                "ASTRA_VN_STAGE_SESSION_FAILED",
                "presentation session terminated after an execution failure",
            ));
        }
        Ok(())
    }

    pub fn state(&self) -> &ProductStageState {
        &self.state
    }

    pub fn resize_viewport(&mut self, viewport: StageViewport) -> Result<(), VnError> {
        self.ensure_active()?;
        validate_viewport(viewport)?;
        self.validate_state()?;
        self.state.viewport = viewport;
        Ok(())
    }

    pub fn active_timeline_count(&self) -> usize {
        self.timelines.len()
    }

    pub fn requires_frame_tick(&self) -> bool {
        !self.is_failed()
            && (!self.tweens.is_empty()
                || !self.timelines.is_empty()
                || self.shake.is_some()
                || self
                    .coordinator
                    .state()
                    .text
                    .active
                    .as_ref()
                    .is_some_and(|text| !text.reveal_complete()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn configure_text(
        &mut self,
        command_id: String,
        text_key: String,
        speaker: Option<String>,
        window: Option<String>,
        grapheme_count: u32,
        graphemes_per_second: u16,
        interrupt: PresentationInterruptPolicy,
    ) -> Result<(), VnError> {
        self.ensure_active()?;
        let sequence = self.next_region_sequence;
        let next_sequence = self.next_region_sequence.checked_add(1).ok_or_else(|| {
            stage_error(
                "ASTRA_VN_STAGE_REGION_SEQUENCE",
                "presentation region sequence overflowed",
            )
        })?;
        self.coordinator.apply_batch(
            &[PresentationCommandEnvelope {
                fixed_step: self.state.frame_index,
                sequence,
                command_id,
                interrupt,
                fence: None,
                payload: PresentationRegionCommand::Text(TextRegionCommand {
                    text_key,
                    speaker,
                    window,
                    grapheme_count,
                    graphemes_per_second,
                }),
            }],
            1,
        )?;
        self.next_region_sequence = next_sequence;
        Ok(())
    }

    pub fn text_reveal_state(&self) -> Option<VnTextRevealState> {
        self.coordinator
            .state()
            .text
            .active
            .as_ref()
            .map(|text| VnTextRevealState {
                command_id: text.command_id.clone(),
                text_key: text.text_key.clone(),
                text_graphemes: text.grapheme_count,
                visible_graphemes: text.visible_graphemes,
                elapsed_ns: text.elapsed_ns,
                graphemes_per_second: text.graphemes_per_second,
            })
    }

    pub fn request_text_advance(&mut self) -> TextAdvanceDisposition {
        if self.is_failed() {
            return TextAdvanceDisposition::NoActiveText;
        }
        self.coordinator.request_text_advance()
    }

    pub fn complete_text_layout(&mut self, command_id: &str) -> Result<(), VnError> {
        self.ensure_active()?;
        self.coordinator.complete_text_layout(command_id)
    }

    pub fn acknowledge_story_advance(&mut self) -> Result<(), VnError> {
        self.ensure_active()?;
        self.coordinator.acknowledge_story_advance()
    }

    pub fn start_video(&mut self, layer: &str) -> Result<(), VnError> {
        self.ensure_active()?;
        self.coordinator.start_video(&format!("movie.{layer}"))
    }

    pub fn fence_status(&self, id: &str) -> Option<crate::FenceStatus> {
        self.coordinator.state().fences.get(id).copied()
    }

    pub fn complete_video(&mut self, layer: &str) -> Result<Vec<String>, VnError> {
        self.ensure_active()?;
        self.coordinator.complete_video(&format!("movie.{layer}"))
    }

    pub fn fail_video(&mut self, layer: &str) -> Result<Option<String>, VnError> {
        self.ensure_active()?;
        self.coordinator.fail_video(&format!("movie.{layer}"))
    }

    pub fn apply(&mut self, command: &StageCommand) -> Result<Vec<StageDirectorOutput>, VnError> {
        let (next, mut outputs) = self.prepare_batch(std::iter::once(command))?;
        *self = next;
        Ok(outputs
            .pop()
            .expect("single command produces one output group"))
    }

    pub fn prepare_batch<'a>(
        &self,
        commands: impl IntoIterator<Item = &'a StageCommand>,
    ) -> Result<(Self, Vec<Vec<StageDirectorOutput>>), VnError> {
        self.ensure_active()?;
        let mut next = self.clone();
        let mut outputs = Vec::new();
        for command in commands {
            let Some(envelope) = next.region_envelope(command)? else {
                outputs.push(next.apply_inner(command)?);
                continue;
            };
            let command_id = envelope.command_id.clone();
            let deltas = next.coordinator.apply_batch(&[envelope], 1)?;
            let applied = deltas
                .iter()
                .any(|delta| delta.applied_commands.iter().any(|id| id == &command_id));
            if applied {
                outputs.push(next.apply_inner(command)?);
            } else {
                if next
                    .queued_region_commands
                    .insert(command_id, command.clone())
                    .is_some()
                {
                    return Err(stage_error(
                        "ASTRA_VN_STAGE_REGION_QUEUE_DUPLICATE",
                        "presentation coordinator returned a duplicate queued command id",
                    ));
                }
                outputs.push(Vec::new());
            }
        }
        next.validate_state()?;
        Ok((next, outputs))
    }

    pub fn tick(&mut self, delta_ns: u64) -> Result<Vec<StageDirectorOutput>, VnError> {
        self.ensure_active()?;
        if delta_ns == 0 || delta_ns > MAX_FRAME_DELTA_NS {
            return Err(stage_error(
                "ASTRA_VN_STAGE_TICK_DELTA",
                "presentation frame delta is outside the fixed-step budget",
            ));
        }
        self.state.frame_index.checked_add(1).ok_or_else(|| {
            stage_error("ASTRA_VN_STAGE_FRAME_OVERFLOW", "frame index overflowed")
        })?;
        self.state
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or_else(|| stage_error("ASTRA_VN_STAGE_TIME_OVERFLOW", "stage time overflowed"))?;
        let result = self.tick_committed(delta_ns);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(super) fn tick_committed(
        &mut self,
        delta_ns: u64,
    ) -> Result<Vec<StageDirectorOutput>, VnError> {
        let mut output = self.tick_inner(delta_ns)?;
        output.extend(self.coordinator.tick(delta_ns)?.into_iter().map(|id| {
            StageDirectorOutput::FenceCompleted {
                kind: "presentation.region".to_string(),
                id,
            }
        }));
        for command_id in self.coordinator.take_activated_commands() {
            let command = self
                .queued_region_commands
                .remove(&command_id)
                .ok_or_else(|| {
                    stage_error(
                        "ASTRA_VN_STAGE_REGION_QUEUE_STATE",
                        "presentation coordinator activated an unknown stage command",
                    )
                })?;
            output.extend(self.apply_inner(&command)?);
        }
        let queued = std::mem::take(&mut self.queued_stage_commands);
        for command in queued {
            output.extend(self.apply_inner(&command)?);
        }
        Ok(output)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, VnError> {
        self.ensure_active()?;
        postcard::to_allocvec(self).map_err(Into::into)
    }

    pub fn restore(
        manifest: VnPresentationProviderManifest,
        profile: &str,
        bytes: &[u8],
    ) -> Result<Self, VnError> {
        let (_, state_bytes) = postcard::take_from_bytes::<VnPresentationProviderManifest>(bytes)?;
        let (schema, _) = postcard::take_from_bytes::<String>(state_bytes)?;
        if schema != PRODUCT_STAGE_STATE_SCHEMA {
            return Err(stage_error(
                "ASTRA_VN_STAGE_SNAPSHOT_IDENTITY",
                "unsupported stage snapshot; retain the old save and create a new slot",
            ));
        }
        let restored: Self = postcard::from_bytes(bytes)?;
        manifest.profile(profile).map_err(VnError::Diagnostic)?;
        if restored.state.schema != PRODUCT_STAGE_STATE_SCHEMA
            || restored.state.profile != profile
            || restored.manifest != manifest
        {
            return Err(stage_error(
                "ASTRA_VN_STAGE_SNAPSHOT_IDENTITY",
                "presentation snapshot identity does not match the package binding",
            ));
        }
        restored.ensure_active()?;
        restored.coordinator.validate_restored_state()?;
        restored.validate_state()?;
        Ok(restored)
    }

    /// Validate a decoded candidate against this session before replacing live state.
    pub fn validate_restore_candidate(&self, candidate: &Self) -> Result<(), VnError> {
        if candidate.state.schema != PRODUCT_STAGE_STATE_SCHEMA
            || candidate.state.profile != self.state.profile
            || candidate.manifest != self.manifest
        {
            return Err(stage_error(
                "ASTRA_VN_STAGE_SNAPSHOT_IDENTITY",
                "presentation snapshot identity does not match the active session",
            ));
        }
        candidate.ensure_active()?;
        candidate.coordinator.validate_restored_state()?;
        candidate.validate_state()
    }
}
