use super::*;

impl PresentationCoordinator {
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    pub(super) fn ensure_active(&self) -> Result<(), VnError> {
        if self.failed {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_SESSION_FAILED",
                "presentation session terminated after an execution failure",
            ));
        }
        Ok(())
    }

    pub fn state(&self) -> &PresentationCoordinatorState {
        &self.state
    }

    pub fn prepare_batch(
        &self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<(Self, Vec<PresentationRegionDelta>), VnError> {
        self.ensure_active()?;
        WorkerBudgetBroker::global()
            .run_scoped(|| self.prepare_batch_scoped(commands, worker_count))
            .map_err(|error| coordinator_error_owned(error.code(), error.to_string()))?
    }

    pub(super) fn prepare_batch_scoped(
        &self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<(Self, Vec<PresentationRegionDelta>), VnError> {
        if !(1..=8).contains(&worker_count) {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_WORKER_COUNT",
                "presentation worker count must be within 1..=8",
            ));
        }
        validate_batch(&self.state, commands)?;
        let mut grouped: BTreeMap<PresentationRegion, Vec<PresentationCommandEnvelope>> =
            BTreeMap::new();
        for command in commands {
            grouped
                .entry(command.region())
                .or_default()
                .push(command.clone());
        }

        let character = self.state.character.clone();
        let background = self.state.background.clone();
        let text = self.state.text.clone();
        let video = self.state.video.clone();
        let execute = |region: PresentationRegion,
                       commands: Vec<PresentationCommandEnvelope>|
         -> Result<PreparedRegion, VnError> {
            match region {
                PresentationRegion::Character => prepare_character(character.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Character(state, delta)),
                PresentationRegion::Background => prepare_background(background.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Background(state, delta)),
                PresentationRegion::Text => prepare_text(text.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Text(Box::new(state), delta)),
                PresentationRegion::Video => prepare_video(video.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Video(state, delta)),
            }
        };

        let prepared = if worker_count == 1 || grouped.len() <= 1 {
            grouped
                .into_iter()
                .map(|(region, commands)| execute(region, commands))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let jobs = grouped.into_iter().collect::<Vec<_>>();
            let mut leases = Vec::new();
            for _ in 1..worker_count.min(jobs.len()) {
                match WorkerBudgetBroker::global()
                    .try_acquire()
                    .map_err(|error| coordinator_error_owned(error.code(), error.to_string()))?
                {
                    Some(lease) => leases.push(lease),
                    None => break,
                }
            }
            let workers = (leases.len() + 1).min(jobs.len());
            let chunk_size = jobs.len().div_ceil(workers);
            std::thread::scope(|scope| {
                let mut chunks = jobs
                    .chunks(chunk_size)
                    .map(<[_]>::to_vec)
                    .collect::<Vec<_>>();
                let caller_chunk = chunks.remove(0);
                let handles = chunks
                    .into_iter()
                    .zip(leases)
                    .map(|(chunk, lease)| {
                        scope.spawn(move || {
                            let _lease = lease;
                            chunk
                                .iter()
                                .cloned()
                                .map(|(region, commands)| execute(region, commands))
                                .collect::<Result<Vec<_>, _>>()
                        })
                    })
                    .collect::<Vec<_>>();
                let mut prepared = caller_chunk
                    .into_iter()
                    .map(|(region, commands)| execute(region, commands))
                    .collect::<Result<Vec<_>, _>>()?;
                for handle in handles {
                    let mut chunk = handle.join().map_err(|_| {
                        coordinator_error(
                            "ASTRA_VN_PRESENTATION_WORKER_PANIC",
                            "presentation region worker panicked",
                        )
                    })??;
                    prepared.append(&mut chunk);
                }
                Ok::<Vec<PreparedRegion>, VnError>(prepared)
            })?
        };

        let mut next = self.clone();
        let mut deltas = Vec::new();
        for region in prepared {
            let delta = match region {
                PreparedRegion::Character(state, delta) => {
                    next.state.character = state;
                    delta
                }
                PreparedRegion::Background(state, delta) => {
                    next.state.background = state;
                    delta
                }
                PreparedRegion::Text(state, delta) => {
                    next.state.text = *state;
                    delta
                }
                PreparedRegion::Video(state, delta) => {
                    next.state.video = state;
                    delta
                }
            };
            deltas.push(delta);
        }
        next.register_fences(&self.state, commands);
        let completed = next.finish_fences(
            deltas
                .iter()
                .flat_map(|delta| delta.completed_fences.iter().cloned())
                .collect(),
        );
        for delta in &mut deltas {
            delta
                .completed_fences
                .retain(|fence| completed.contains(fence));
        }
        deltas.sort_by_key(|delta| (delta.first_sequence, delta.region));
        if let Some(command) = commands.last() {
            next.state.last_fixed_step = command.fixed_step;
            next.state.last_sequence = Some(command.sequence);
        }
        Ok((next, deltas))
    }

    pub fn apply_batch(
        &mut self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<Vec<PresentationRegionDelta>, VnError> {
        let (next, deltas) = self.prepare_batch(commands, worker_count)?;
        *self = next;
        Ok(deltas)
    }

    pub fn tick(&mut self, delta_ns: u64) -> Result<Vec<String>, VnError> {
        self.ensure_active()?;
        if delta_ns == 0 || delta_ns > MAX_FRAME_DELTA_NS {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_TICK_DELTA",
                "presentation frame delta is outside the fixed-step budget",
            ));
        }
        let result = self.tick_committed(delta_ns);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(super) fn tick_committed(&mut self, delta_ns: u64) -> Result<Vec<String>, VnError> {
        let mut completed = Vec::new();
        for state in self.state.character.characters.values_mut() {
            state.elapsed_ns = state
                .elapsed_ns
                .saturating_add(delta_ns)
                .min(state.duration_ns);
            if state.elapsed_ns == state.duration_ns {
                if let Some(fence) = state.fence.take() {
                    completed.push(fence);
                }
            }
        }
        for state in self.state.background.layers.values_mut() {
            state.elapsed_ns = state
                .elapsed_ns
                .saturating_add(delta_ns)
                .min(state.duration_ns);
            if state.elapsed_ns == state.duration_ns && state.transition_pending {
                state.current = state.incoming.take();
                state.transition_pending = false;
                if let Some(fence) = state.fence.take() {
                    completed.push(fence);
                }
            }
        }
        if let Some(text) = self.state.text.active.as_mut() {
            text.elapsed_ns = text.elapsed_ns.saturating_add(delta_ns);
            let visible = (u128::from(text.elapsed_ns) * u128::from(text.graphemes_per_second)
                / 1_000_000_000_u128)
                .min(u128::from(text.grapheme_count)) as u32;
            text.visible_graphemes = text.visible_graphemes.max(visible);
            if text.reveal_complete() {
                text.tasks.finish_reveal(0)?;
                if let Some(fence) = text.fence.take() {
                    completed.push(fence);
                }
            }
        }
        for video in self.state.video.sessions.values_mut() {
            if video.phase == VideoPhase::Playing {
                video.logical_time_ns = video.logical_time_ns.saturating_add(delta_ns);
            }
        }
        let (character, activated_character) =
            drain_character_queue(std::mem::take(&mut self.state.character))?;
        self.state.character = character;
        let (background, activated_background) =
            drain_background_queue(std::mem::take(&mut self.state.background))?;
        self.state.background = background;
        let (video, activated_video) = drain_video_queue(std::mem::take(&mut self.state.video))?;
        self.state.video = video;
        for command_id in activated_character
            .into_iter()
            .chain(activated_background)
            .chain(activated_video)
        {
            if let Err(error) = push_activated(&mut self.state.activated_commands, command_id) {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(self.finish_fences(completed))
    }

    pub fn request_text_advance(&mut self) -> TextAdvanceDisposition {
        if self.failed {
            return TextAdvanceDisposition::NoActiveText;
        }
        let Some(text) = self.state.text.active.as_mut() else {
            return TextAdvanceDisposition::NoActiveText;
        };
        if !text.reveal_complete() {
            text.visible_graphemes = text.grapheme_count;
            // This click only finishes reveal; it is not reused for story advance.
            text.tasks
                .finish_reveal(1)
                .expect("active text task state was validated");
            if let Some(fence) = text.fence.take() {
                self.finish_fences(vec![fence]);
            }
            TextAdvanceDisposition::RevealCompleted
        } else {
            TextAdvanceDisposition::StoryAdvanceRequested
        }
    }

    pub fn complete_text_layout(&mut self, command_id: &str) -> Result<(), VnError> {
        self.ensure_active()?;
        let text = self.state.text.active.as_mut().ok_or_else(|| {
            coordinator_error(
                "ASTRA_VN_TEXT_LAYOUT_STATE",
                "text layout completion references an empty text region",
            )
        })?;
        if text.command_id != command_id {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_LAYOUT_IDENTITY",
                "text layout completion does not match the active command",
            ));
        }
        text.layout_pending = false;
        Ok(())
    }

    pub fn acknowledge_story_advance(&mut self) -> Result<(), VnError> {
        self.ensure_active()?;
        let Some(active) = self.state.text.active.as_ref() else {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_ADVANCE_STATE",
                "story advance cannot acknowledge an empty text region",
            ));
        };
        if !active.reveal_complete() {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_ADVANCE_STATE",
                "story advance cannot acknowledge an incomplete text reveal",
            ));
        }
        self.state
            .text
            .active
            .as_mut()
            .expect("validated active text")
            .tasks
            .acknowledge()?;
        self.state.text.active = None;
        let (text, activated) = drain_text_queue(std::mem::replace(
            &mut self.state.text,
            TextRegionState {
                active: None,
                queued: VecDeque::new(),
            },
        ))?;
        self.state.text = text;
        for command_id in activated {
            if let Err(error) = push_activated(&mut self.state.activated_commands, command_id) {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn take_activated_commands(&mut self) -> Vec<String> {
        if self.failed {
            return Vec::new();
        }
        self.state.activated_commands.drain(..).collect()
    }

    pub fn start_video(&mut self, session_id: &str) -> Result<(), VnError> {
        self.ensure_active()?;
        let video = self
            .state
            .video
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| {
                coordinator_error(
                    "ASTRA_VN_VIDEO_SESSION_MISSING",
                    "video start references an unknown presentation session",
                )
            })?;
        match video.phase {
            VideoPhase::Prebuffering => video.phase = VideoPhase::Playing,
            VideoPhase::Playing => {}
            _ => {
                return Err(coordinator_error(
                    "ASTRA_VN_VIDEO_PHASE",
                    "decoded video frames are accepted only while prebuffering or playing",
                ));
            }
        }
        Ok(())
    }

    pub fn complete_video(&mut self, session_id: &str) -> Result<Vec<String>, VnError> {
        self.ensure_active()?;
        let video = self
            .state
            .video
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| {
                coordinator_error(
                    "ASTRA_VN_VIDEO_SESSION_MISSING",
                    "video completion references an unknown presentation session",
                )
            })?;
        video.phase = VideoPhase::Ended;
        let mut completed = Vec::new();
        if let Some(fence) = video.fence.take() {
            completed.push(fence);
        }
        let (video, activated) = drain_video_queue(std::mem::replace(
            &mut self.state.video,
            VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
        ))?;
        self.state.video = video;
        for command_id in activated {
            if let Err(error) = push_activated(&mut self.state.activated_commands, command_id) {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(self.finish_fences(completed))
    }

    pub fn fail_video(&mut self, session_id: &str) -> Result<Option<String>, VnError> {
        self.ensure_active()?;
        let fallback = {
            let video = self
                .state
                .video
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| {
                    coordinator_error(
                        "ASTRA_VN_VIDEO_SESSION_MISSING",
                        "video failure references an unknown presentation session",
                    )
                })?;
            video.phase = VideoPhase::Failed;
            if let Some(fence) = video.fence.take() {
                if let Some(group) = self.state.fence_tasks.get_mut(&fence) {
                    group.cancel();
                }
                self.state.fences.insert(fence, FenceStatus::Failed);
            }
            video.fallback.clone()
        };
        let state = std::mem::replace(
            &mut self.state.video,
            VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
        );
        let (video, activated) = drain_video_queue(state)?;
        self.state.video = video;
        for command_id in activated {
            if let Err(error) = push_activated(&mut self.state.activated_commands, command_id) {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(fallback)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, VnError> {
        self.ensure_active()?;
        postcard::to_allocvec(self).map_err(Into::into)
    }

    pub fn restore(bytes: &[u8]) -> Result<Self, VnError> {
        let (schema, _) = postcard::take_from_bytes::<String>(bytes)?;
        if schema != PRESENTATION_COORDINATOR_SCHEMA {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_SNAPSHOT_SCHEMA",
                "unsupported presentation snapshot; retain the old save and create a new slot",
            ));
        }
        let restored: Self = postcard::from_bytes(bytes)?;
        restored.validate_restored_state()?;
        Ok(restored)
    }
}
