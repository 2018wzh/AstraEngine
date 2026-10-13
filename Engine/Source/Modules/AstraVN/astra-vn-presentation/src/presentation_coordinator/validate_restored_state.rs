use super::*;

impl PresentationCoordinator {
    pub(crate) fn validate_restored_state(&self) -> Result<(), VnError> {
        if self.state.schema != PRESENTATION_COORDINATOR_SCHEMA {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_SNAPSHOT_SCHEMA",
                "presentation coordinator snapshot schema is invalid",
            ));
        }
        self.ensure_active()?;
        validate_queues(&self.state)?;
        if let Some(text) = &self.state.text.active {
            text.tasks.validate(text.reveal_complete())?;
        }
        self.validate_fences()
    }
}

pub(super) enum PreparedRegion {
    Character(CharacterRegionState, PresentationRegionDelta),
    Background(BackgroundRegionState, PresentationRegionDelta),
    Text(Box<TextRegionState>, PresentationRegionDelta),
    Video(VideoRegionState, PresentationRegionDelta),
}

pub(super) fn validate_batch(
    state: &PresentationCoordinatorState,
    commands: &[PresentationCommandEnvelope],
) -> Result<(), VnError> {
    fences::validate_commands(state, commands)?;
    let mut prior = state.last_sequence;
    let mut layer_regions: BTreeMap<&str, PresentationRegion> = BTreeMap::new();
    for command in commands {
        if command.command_id.is_empty()
            || command.fence.as_ref().is_some_and(String::is_empty)
            || prior.is_some_and(|value| command.sequence <= value)
            || command.fixed_step < state.last_fixed_step
        {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_COMMAND_ORDER",
                "presentation command identity, sequence or fixed step is invalid",
            ));
        }
        prior = Some(command.sequence);
        if let Some(layer) = command.layer_write() {
            if let Some(previous) = layer_regions.insert(layer, command.region()) {
                if previous != command.region() {
                    return Err(coordinator_error(
                        "ASTRA_VN_PRESENTATION_REGION_WRITE_CONFLICT",
                        "multiple presentation regions write the same layer in one transaction",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_queues(state: &PresentationCoordinatorState) -> Result<(), VnError> {
    for (region, queue) in [
        (PresentationRegion::Character, &state.character.queued),
        (PresentationRegion::Background, &state.background.queued),
        (PresentationRegion::Text, &state.text.queued),
        (PresentationRegion::Video, &state.video.queued),
    ] {
        for command in queue {
            if command.region() != region
                || command.interrupt != PresentationInterruptPolicy::Queue
                || command.command_id.is_empty()
                || matches!(&command.payload, PresentationRegionCommand::Text(text) if text.graphemes_per_second == 0)
            {
                return Err(coordinator_error(
                    "ASTRA_VN_PRESENTATION_QUEUE_STATE",
                    "queued presentation command violates its region or input contract",
                ));
            }
        }
    }
    if [
        state.character.queued.len(),
        state.background.queued.len(),
        state.text.queued.len(),
        state.video.queued.len(),
        state.activated_commands.len(),
    ]
    .into_iter()
    .any(|len| len > MAX_REGION_QUEUE)
    {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_QUEUE_LIMIT",
            "presentation region queue exceeds its serialized bound",
        ));
    }
    Ok(())
}

pub(super) fn prepare_character(
    mut state: CharacterRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(CharacterRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Character(command) = &envelope.payload else {
            unreachable!("region classifier guarantees character payloads");
        };
        let active = state
            .characters
            .get(&command.character_id)
            .is_some_and(|value| value.elapsed_ns < value.duration_ns);
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_CHARACTER_INTERRUPT_REJECTED",
                        "character command conflicts with an active transition",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.characters.insert(
            command.character_id.clone(),
            CharacterPresentationState {
                command_id: envelope.command_id.clone(),
                asset: command.asset.clone(),
                pose: command.pose.clone(),
                layer: command.layer.clone(),
                visible: command.visible,
                elapsed_ns: 0,
                duration_ns: command.duration_ns,
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Character,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

pub(super) fn prepare_background(
    mut state: BackgroundRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(BackgroundRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Background(command) = &envelope.payload else {
            unreachable!("region classifier guarantees background payloads");
        };
        let existing = state.layers.get(&command.layer).cloned();
        let active = existing
            .as_ref()
            .is_some_and(|value| value.elapsed_ns < value.duration_ns);
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_BACKGROUND_INTERRUPT_REJECTED",
                        "background command conflicts with an active transition",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        let current = existing.and_then(|value| match envelope.interrupt {
            PresentationInterruptPolicy::SnapThenStart => value.incoming.or(value.current),
            _ => value.current,
        });
        state.layers.insert(
            command.layer.clone(),
            BackgroundPresentationState {
                command_id: envelope.command_id.clone(),
                current,
                incoming: command.asset.clone(),
                transition_pending: true,
                elapsed_ns: 0,
                duration_ns: command.duration_ns,
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Background,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

pub(super) fn prepare_text(
    mut state: TextRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(TextRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Text(command) = &envelope.payload else {
            unreachable!("region classifier guarantees text payloads");
        };
        if command.graphemes_per_second == 0 {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_REVEAL_RATE",
                "text reveal rate must be non-zero",
            ));
        }
        if state.active.is_some() {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_TEXT_INTERRUPT_REJECTED",
                        "text command conflicts with an active line",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.active = Some(TextPresentationState {
            command_id: envelope.command_id.clone(),
            text_key: command.text_key.clone(),
            speaker: command.speaker.clone(),
            window: command.window.clone(),
            grapheme_count: command.grapheme_count,
            visible_graphemes: 0,
            elapsed_ns: 0,
            graphemes_per_second: command.graphemes_per_second,
            layout_pending: true,
            auto_timer_ns: None,
            tasks: TextTaskProgress::new(command.grapheme_count == 0),
            fence: envelope.fence.clone(),
        });
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Text,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

pub(super) fn prepare_video(
    mut state: VideoRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(VideoRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Video(command) = &envelope.payload else {
            unreachable!("region classifier guarantees video payloads");
        };
        let active = state
            .sessions
            .get(&command.session_id)
            .is_some_and(|video| !matches!(video.phase, VideoPhase::Ended | VideoPhase::Failed));
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_VIDEO_INTERRUPT_REJECTED",
                        "video command conflicts with an active playback session",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.sessions.insert(
            command.session_id.clone(),
            VideoPresentationState {
                command_id: envelope.command_id.clone(),
                layer: command.layer.clone(),
                asset: command.asset.clone(),
                phase: VideoPhase::Prebuffering,
                logical_time_ns: command.logical_start_ns,
                loop_mode: command.loop_mode,
                end_behavior: command.end_behavior,
                fallback: command.fallback.clone(),
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Video,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

pub(super) fn push_queued(
    queue: &mut VecDeque<PresentationCommandEnvelope>,
    command: PresentationCommandEnvelope,
) -> Result<(), VnError> {
    if queue.len() >= MAX_REGION_QUEUE {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_QUEUE_LIMIT",
            "presentation region queue is full",
        ));
    }
    queue.push_back(command);
    Ok(())
}

pub(super) fn drain_character_queue(
    mut state: CharacterRegionState,
) -> Result<(CharacterRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_character(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

pub(super) fn drain_background_queue(
    mut state: BackgroundRegionState,
) -> Result<(BackgroundRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_background(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

pub(super) fn drain_text_queue(
    mut state: TextRegionState,
) -> Result<(TextRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_text(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

pub(super) fn drain_video_queue(
    mut state: VideoRegionState,
) -> Result<(VideoRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_video(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

pub(super) fn push_activated(
    queue: &mut VecDeque<String>,
    command_id: String,
) -> Result<(), VnError> {
    if queue.len() >= MAX_REGION_QUEUE {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_ACTIVATED_LIMIT",
            "presentation activated-command queue is full",
        ));
    }
    queue.push_back(command_id);
    Ok(())
}

pub(super) fn coordinator_error(code: &'static str, message: &'static str) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}

pub(super) fn coordinator_error_owned(code: &'static str, message: String) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}
