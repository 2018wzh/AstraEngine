//! Explicit task progress; snapshots never contain futures, workers, or IO callbacks.
use crate::TaskScope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TaskGroupMode {
    Sequence,
    All,
    Race,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TaskTerminal {
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TaskMemberState {
    Waiting,
    Running,
    Terminal(TaskTerminal),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskGroupState {
    mode: TaskGroupMode,
    members: Vec<TaskMemberState>,
    terminal: Option<TaskTerminal>,
    winner: Option<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskGroupError {
    #[error("task group requires at least one member")]
    Empty,
    #[error("task completion is duplicate, inactive, or out of range")]
    Inactive,
    #[error("task completion belongs to a cancelled or replaced scope")]
    Stale,
    #[error("saved task group state is inconsistent")]
    InvalidSnapshot,
}
impl TaskGroupState {
    pub fn new(mode: TaskGroupMode, count: usize) -> Result<Self, TaskGroupError> {
        if count == 0 {
            return Err(TaskGroupError::Empty);
        }
        let mut members = vec![TaskMemberState::Running; count];
        if mode == TaskGroupMode::Sequence {
            members[1..].fill(TaskMemberState::Waiting);
        }
        Ok(Self {
            mode,
            members,
            terminal: None,
            winner: None,
        })
    }
    pub fn members(&self) -> &[TaskMemberState] {
        &self.members
    }
    pub fn terminal(&self) -> Option<TaskTerminal> {
        self.terminal
    }
    pub fn winner(&self) -> Option<usize> {
        self.winner
    }
    pub fn mode(&self) -> TaskGroupMode {
        self.mode
    }
    /// Only Running members may be started. A sequence exposes its next member
    /// after the previous completion is accepted, never when the group is created.
    pub fn active(&self) -> impl Iterator<Item = usize> + '_ {
        self.members
            .iter()
            .enumerate()
            .filter_map(|(i, s)| (*s == TaskMemberState::Running).then_some(i))
    }
    /// All waits for every terminal result. Failure takes precedence over cancellation.
    /// Race accepts the first terminal result, including failure/cancellation.
    pub fn complete(&mut self, member: usize, result: TaskTerminal) -> Result<(), TaskGroupError> {
        if self.terminal.is_some() || self.members.get(member) != Some(&TaskMemberState::Running) {
            return Err(TaskGroupError::Inactive);
        }
        self.members[member] = TaskMemberState::Terminal(result);
        match self.mode {
            TaskGroupMode::Sequence if result == TaskTerminal::Completed => {
                if member + 1 == self.members.len() {
                    self.terminal = Some(result);
                } else {
                    self.members[member + 1] = TaskMemberState::Running;
                }
            }
            TaskGroupMode::Sequence | TaskGroupMode::Race => {
                if self.mode == TaskGroupMode::Race {
                    self.winner = Some(member);
                }
                self.finish(result);
            }
            TaskGroupMode::All => {
                if self.active().next().is_none() {
                    let result = if self
                        .members
                        .contains(&TaskMemberState::Terminal(TaskTerminal::Failed))
                    {
                        TaskTerminal::Failed
                    } else if self
                        .members
                        .contains(&TaskMemberState::Terminal(TaskTerminal::Cancelled))
                    {
                        TaskTerminal::Cancelled
                    } else {
                        TaskTerminal::Completed
                    };
                    self.terminal = Some(result);
                }
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        if self.terminal.is_none() {
            self.finish(TaskTerminal::Cancelled);
        }
    }
    fn finish(&mut self, result: TaskTerminal) {
        self.terminal = Some(result);
        for member in &mut self.members {
            if matches!(member, TaskMemberState::Waiting | TaskMemberState::Running) {
                *member = TaskMemberState::Terminal(TaskTerminal::Cancelled);
            }
        }
    }
    pub fn validate(&self) -> Result<(), TaskGroupError> {
        if self.members.is_empty() {
            return Err(TaskGroupError::InvalidSnapshot);
        }
        let mut reconstructed = Self::new(self.mode, self.members.len())?;
        if self.terminal == Some(TaskTerminal::Cancelled) && self.winner.is_none() {
            for (i, member) in self.members.iter().enumerate() {
                if let TaskMemberState::Terminal(
                    result @ (TaskTerminal::Completed | TaskTerminal::Failed),
                ) = member
                {
                    if reconstructed.terminal.is_none() {
                        reconstructed.complete(i, *result)?;
                    }
                }
            }
            reconstructed.cancel();
        } else if self.mode == TaskGroupMode::Race {
            if let Some(winner) = self.winner {
                let Some(TaskMemberState::Terminal(result)) = self.members.get(winner) else {
                    return Err(TaskGroupError::InvalidSnapshot);
                };
                reconstructed.complete(winner, *result)?;
            }
        } else {
            for (i, member) in self.members.iter().enumerate() {
                if let TaskMemberState::Terminal(result) = member {
                    if reconstructed.terminal.is_none() {
                        reconstructed.complete(i, *result)?;
                    }
                }
            }
        }
        if reconstructed != *self
            && self.terminal == Some(TaskTerminal::Cancelled)
            && self.winner.is_none()
        {
            reconstructed.cancel();
        }
        if reconstructed == *self {
            Ok(())
        } else {
            Err(TaskGroupError::InvalidSnapshot)
        }
    }
}

/// Process-local owner. Dropping/replacing the group invalidates its completion handles.
/// Worker owners must observe cancellation and join their workers before releasing resources.
/// Restoring this owner only binds explicit progress to a fresh scope; it starts no IO.
pub struct TaskGroup {
    state: TaskGroupState,
    scope: TaskScope,
    members: Vec<TaskScope>,
}
#[derive(Clone)]
pub struct TaskGroupHandle {
    index: usize,
    scope: TaskScope,
}
impl TaskGroupHandle {
    pub fn scope(&self) -> &TaskScope {
        &self.scope
    }
    pub fn index(&self) -> usize {
        self.index
    }
}
impl TaskGroup {
    pub fn new(
        parent: &TaskScope,
        mode: TaskGroupMode,
        count: usize,
    ) -> Result<Self, TaskGroupError> {
        Self::restore(parent, TaskGroupState::new(mode, count)?)
    }
    pub fn restore(parent: &TaskScope, state: TaskGroupState) -> Result<Self, TaskGroupError> {
        state.validate()?;
        let scope = parent.child();
        let members = state
            .members
            .iter()
            .map(|state| {
                let child = scope.child();
                if matches!(state, TaskMemberState::Terminal(_)) {
                    child.cancel();
                }
                child
            })
            .collect();
        Ok(Self {
            state,
            scope,
            members,
        })
    }
    pub fn state(&self) -> &TaskGroupState {
        &self.state
    }
    pub fn snapshot(&self) -> TaskGroupState {
        let mut state = self.state.clone();
        if self.scope.is_cancelled() {
            state.cancel();
        }
        state
    }
    pub fn handle(&self, index: usize) -> Result<TaskGroupHandle, TaskGroupError> {
        if self.scope.is_cancelled() {
            return Err(TaskGroupError::Stale);
        }
        if self.state.members.get(index) != Some(&TaskMemberState::Running) {
            return Err(TaskGroupError::Inactive);
        }
        Ok(TaskGroupHandle {
            index,
            scope: self.members[index].clone(),
        })
    }
    pub fn complete(
        &mut self,
        handle: &TaskGroupHandle,
        result: TaskTerminal,
    ) -> Result<(), TaskGroupError> {
        if self.scope.is_cancelled()
            || handle.scope.is_cancelled()
            || self.members.get(handle.index) != Some(&handle.scope)
        {
            return Err(TaskGroupError::Stale);
        }
        self.state.complete(handle.index, result)?;
        for (scope, state) in self.members.iter().zip(&self.state.members) {
            if matches!(state, TaskMemberState::Terminal(_)) {
                scope.cancel();
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.scope.cancel();
        self.state.cancel();
    }
}
impl Drop for TaskGroup {
    fn drop(&mut self) {
        self.scope.cancel();
    }
}
