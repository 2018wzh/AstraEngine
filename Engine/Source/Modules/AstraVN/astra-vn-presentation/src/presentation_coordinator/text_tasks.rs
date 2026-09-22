use super::*;
use astra_runtime::{TaskGroupMode, TaskGroupState, TaskTerminal};

/// Reveal (clock or click), then a distinct story acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextTaskProgress {
    sequence: TaskGroupState,
    reveal: TaskGroupState,
}
impl TextTaskProgress {
    pub(super) fn new(revealed: bool) -> Self {
        let mut value = Self {
            sequence: TaskGroupState::new(TaskGroupMode::Sequence, 2).expect("two tasks"),
            reveal: TaskGroupState::new(TaskGroupMode::Race, 2).expect("two tasks"),
        };
        if revealed {
            value.finish_reveal(0).expect("fresh reveal");
        }
        value
    }
    pub(super) fn finish_reveal(&mut self, winner: usize) -> Result<(), VnError> {
        if self.reveal.terminal().is_none() {
            self.reveal
                .complete(winner, TaskTerminal::Completed)
                .map_err(task_error)?;
            self.sequence
                .complete(0, TaskTerminal::Completed)
                .map_err(task_error)?;
        }
        Ok(())
    }
    pub(super) fn acknowledge(&mut self) -> Result<(), VnError> {
        self.sequence
            .complete(1, TaskTerminal::Completed)
            .map_err(task_error)
    }
    pub(super) fn validate(&self, revealed: bool) -> Result<(), VnError> {
        self.sequence.validate().map_err(task_error)?;
        self.reveal.validate().map_err(task_error)?;
        if self.sequence.mode() != TaskGroupMode::Sequence
            || self.reveal.mode() != TaskGroupMode::Race
            || self.sequence.members().len() != 2
            || self.reveal.members().len() != 2
            || self.sequence.terminal().is_some()
            || self.reveal.terminal() != revealed.then_some(TaskTerminal::Completed)
            || self.sequence.active().next() != Some(usize::from(revealed))
        {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_TASK_STATE",
                "saved text task progress differs from revealed text",
            ));
        }
        Ok(())
    }
}
fn task_error(_error: astra_runtime::TaskGroupError) -> VnError {
    coordinator_error(
        "ASTRA_VN_TEXT_TASK_STATE",
        "text task completion is inactive or inconsistent",
    )
}
