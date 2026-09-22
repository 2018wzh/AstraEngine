use super::*;
use astra_runtime::{TaskGroupMode, TaskGroupState, TaskMemberState, TaskTerminal};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FenceTaskGroup {
    ids: Vec<String>,
    progress: TaskGroupState,
}

impl FenceTaskGroup {
    pub(super) fn cancel(&mut self) {
        self.progress.cancel();
    }
}

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
    for (fence, group) in &state.fence_tasks {
        if state.fences.get(fence) == Some(&FenceStatus::Pending) {
            identities.extend(group.ids.iter().map(|id| (fence.as_str(), id.as_str())));
        }
    }
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
        let affected: BTreeSet<_> = commands.iter().filter_map(|c| c.fence.clone()).collect();
        for fence in affected {
            let old = if previous.iter().any(|(id, _)| *id == fence) {
                self.state.fence_tasks.get(&fence).cloned()
            } else {
                None
            };
            let mut ids = old.as_ref().map(|g| g.ids.clone()).unwrap_or_default();
            for command in commands.iter().filter(|c| c.fence.as_ref() == Some(&fence)) {
                if !ids.contains(&command.command_id) {
                    ids.push(command.command_id.clone());
                }
            }
            let mut progress = TaskGroupState::new(TaskGroupMode::All, ids.len())
                .expect("fence has incoming members");
            if let Some(old) = old {
                for (index, status) in old.progress.members().iter().enumerate() {
                    if let TaskMemberState::Terminal(result) = status {
                        progress
                            .complete(index, *result)
                            .expect("distinct fence member");
                    }
                }
            }
            if self.state.fences.get(&fence) == Some(&FenceStatus::Failed) {
                progress.cancel();
            }
            self.state
                .fence_tasks
                .insert(fence, FenceTaskGroup { ids, progress });
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
            if let Some(group) = self.state.fence_tasks.get_mut(&fence) {
                group.progress.cancel();
            }
            self.state.fences.insert(fence, FenceStatus::Failed);
        }
    }

    pub(super) fn finish_fences(&mut self, mut candidates: Vec<String>) -> Vec<String> {
        if candidates.is_empty() {
            return candidates;
        }
        candidates.sort();
        candidates.dedup();
        let pending: Vec<_> = members(&self.state)
            .into_iter()
            .map(|(fence, id)| (fence.to_owned(), id.to_owned()))
            .collect();
        candidates.retain(|fence| {
            if self.state.fences.get(fence) != Some(&FenceStatus::Pending) {
                return false;
            }
            let Some(group) = self.state.fence_tasks.get_mut(fence) else {
                return false;
            };
            let completed: Vec<_> = group
                .progress
                .active()
                .filter(|i| {
                    !pending
                        .iter()
                        .any(|(id, command)| id == fence && command == &group.ids[*i])
                })
                .collect();
            for member in completed {
                group
                    .progress
                    .complete(member, TaskTerminal::Completed)
                    .expect("pending fence member");
            }
            group.progress.terminal() == Some(TaskTerminal::Completed)
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
        for (fence, group) in &self.state.fence_tasks {
            group.progress.validate().map_err(|_| {
                coordinator_error(
                    "ASTRA_VN_PRESENTATION_FENCE_STATE",
                    "saved task group is invalid",
                )
            })?;
            if group.progress.mode() != TaskGroupMode::All
                || group.ids.len() != group.progress.members().len()
                || group.ids.iter().collect::<BTreeSet<_>>().len() != group.ids.len()
            {
                return Err(coordinator_error(
                    "ASTRA_VN_PRESENTATION_FENCE_STATE",
                    "saved fence task members are invalid",
                ));
            }
            let status = self.state.fences.get(fence);
            if status.is_none()
                || (status == Some(&FenceStatus::Completed)
                    && group.progress.terminal() != Some(TaskTerminal::Completed))
                || (status == Some(&FenceStatus::Failed)
                    && group.progress.terminal() != Some(TaskTerminal::Cancelled))
            {
                return Err(coordinator_error(
                    "ASTRA_VN_PRESENTATION_FENCE_STATE",
                    "saved fence outcome differs from its task group",
                ));
            }
            if status == Some(&FenceStatus::Pending) {
                let active: BTreeSet<_> = group
                    .progress
                    .active()
                    .map(|i| group.ids[i].as_str())
                    .collect();
                let actual: BTreeSet<_> = pending
                    .iter()
                    .filter(|(id, _)| *id == fence)
                    .map(|(_, id)| *id)
                    .collect();
                if active != actual || group.progress.terminal().is_some() {
                    return Err(coordinator_error(
                        "ASTRA_VN_PRESENTATION_FENCE_STATE",
                        "saved task progress differs from active fence members",
                    ));
                }
            }
        }
        if self
            .state
            .fences
            .keys()
            .any(|id| !self.state.fence_tasks.contains_key(id))
        {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_FENCE_STATE",
                "saved fence has no task group",
            ));
        }
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
