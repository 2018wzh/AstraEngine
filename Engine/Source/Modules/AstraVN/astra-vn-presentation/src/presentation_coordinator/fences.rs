use super::*;
use std::collections::BTreeSet;

fn members(state: &PresentationCoordinatorState) -> Vec<(&str, &str)> {
    fn insert<'a>(set: &mut Vec<(&'a str, &'a str)>, id: &'a str, fence: &'a Option<String>) {
        if let Some(fence) = fence {
            set.push((fence.as_str(), id));
        }
    }
    let mut set = Vec::new();
    for value in state.character.characters.values() {
        insert(&mut set, &value.command_id, &value.fence);
    }
    for value in state.background.layers.values() {
        insert(&mut set, &value.command_id, &value.fence);
    }
    if let Some(value) = &state.text.active {
        insert(&mut set, &value.command_id, &value.fence);
    }
    for value in state.video.sessions.values() {
        insert(&mut set, &value.command_id, &value.fence);
    }
    for queue in [
        &state.character.queued,
        &state.background.queued,
        &state.text.queued,
        &state.video.queued,
    ] {
        for command in queue {
            insert(&mut set, &command.command_id, &command.fence);
        }
    }
    set
}

pub(super) fn validate_commands(
    state: &PresentationCoordinatorState,
    commands: &[PresentationCommandEnvelope],
) -> Result<(), VnError> {
    let mut identities: BTreeSet<_> = members(state).into_iter().collect();
    for command in commands {
        if let Some(fence) = command.fence.as_deref() {
            if !identities.insert((fence, command.command_id.as_str())) {
                return Err(coordinator_error(
                    "ASTRA_VN_PRESENTATION_FENCE_MEMBER_CONFLICT",
                    "pending fence members require distinct command identities",
                ));
            }
        }
    }
    Ok(())
}

impl PresentationCoordinator {
    pub(super) fn register_fences(
        &mut self,
        before: &PresentationCoordinatorState,
        commands: &[PresentationCommandEnvelope],
    ) {
        let previous = members(before);
        for command in commands {
            if let Some(fence) = &command.fence {
                if !previous.iter().any(|(id, _)| *id == fence) {
                    self.state
                        .fences
                        .insert(fence.clone(), FenceStatus::Pending);
                }
            }
        }
        let current = members(&self.state);
        let lost: BTreeSet<String> = previous
            .into_iter()
            .chain(commands.iter().filter_map(|command| {
                command
                    .fence
                    .as_deref()
                    .map(|fence| (fence, command.command_id.as_str()))
            }))
            .filter(|member| !current.contains(member))
            .map(|(fence, _)| fence.to_string())
            .collect();
        for fence in lost {
            self.state.fences.insert(fence, FenceStatus::Failed);
        }
    }

    pub(super) fn finish_fences(&mut self, mut candidates: Vec<String>) -> Vec<String> {
        if candidates.is_empty() {
            return candidates;
        }
        candidates.sort();
        candidates.dedup();
        let pending = members(&self.state);
        candidates.retain(|fence| {
            self.state.fences.get(fence) == Some(&FenceStatus::Pending)
                && !pending.iter().any(|(id, _)| *id == fence)
        });
        for fence in &candidates {
            self.state
                .fences
                .insert(fence.clone(), FenceStatus::Completed);
        }
        candidates
    }

    pub(super) fn validate_fences(&self) -> Result<(), VnError> {
        let pending = members(&self.state);
        let bad_reference = pending.iter().any(|(fence, _)| {
            !matches!(
                self.state.fences.get(*fence),
                Some(FenceStatus::Pending | FenceStatus::Failed)
            )
        });
        let empty_pending = self.state.fences.iter().any(|(fence, status)| {
            *status == FenceStatus::Pending && !pending.iter().any(|(id, _)| *id == fence)
        });
        if bad_reference
            || empty_pending
            || pending.iter().copied().collect::<BTreeSet<_>>().len() != pending.len()
        {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_FENCE_STATE",
                "saved fence status does not match its active or queued members",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_rejects_premature_completion_and_previous_schema() {
        let mut coordinator = PresentationCoordinator::default();
        coordinator
            .apply_batch(
                &[PresentationCommandEnvelope {
                    fixed_step: 1,
                    sequence: 1,
                    command_id: "background".into(),
                    fence: Some("all".into()),
                    interrupt: PresentationInterruptPolicy::Reject,
                    payload: PresentationRegionCommand::Background(BackgroundRegionCommand {
                        layer: "bg".into(),
                        asset: Some("asset:/room.png".into()),
                        duration_ns: 100,
                    }),
                }],
                1,
            )
            .unwrap();
        for old_schema in [false, true] {
            let mut invalid = coordinator.clone();
            if old_schema {
                invalid.state.schema = "astra.vn.presentation_coordinator.v4".into();
            } else {
                invalid
                    .state
                    .fences
                    .insert("all".into(), FenceStatus::Completed);
            }
            assert!(PresentationCoordinator::restore(&invalid.snapshot().unwrap()).is_err());
        }
    }
}
