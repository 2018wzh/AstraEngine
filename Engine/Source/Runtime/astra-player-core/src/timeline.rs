use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerTimelineTaskAction {
    Start,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerTimelineTask {
    pub schema: String,
    pub task_id: String,
    pub target: Option<String>,
    pub action: PlayerTimelineTaskAction,
    pub duration_ms: Option<u64>,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerTimelineCompletionKind {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerTimelineCompletion {
    pub schema: String,
    pub task_id: String,
    pub target: String,
    pub fence: Option<String>,
    pub kind: PlayerTimelineCompletionKind,
    pub completed_at_ms: u64,
}

#[derive(Debug, Clone)]
struct ActiveTimelineTask {
    task_id: String,
    target: String,
    fence: Option<String>,
    deadline_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerTimelineSchedulerSnapshot {
    pub schema: String,
    pub capacity: usize,
    pub last_time_ms: Option<u64>,
    pub active: Vec<PlayerTimelineTaskSnapshot>,
    pub completed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerTimelineTaskSnapshot {
    pub task_id: String,
    pub target: String,
    pub fence: Option<String>,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PlayerTimelineScheduler {
    capacity: usize,
    last_time_ms: Option<u64>,
    active: BTreeMap<String, ActiveTimelineTask>,
    completed: std::collections::BTreeSet<String>,
}

impl PlayerTimelineScheduler {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            last_time_ms: None,
            active: BTreeMap::new(),
            completed: std::collections::BTreeSet::new(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn snapshot(&self) -> PlayerTimelineSchedulerSnapshot {
        PlayerTimelineSchedulerSnapshot {
            schema: "astra.player_timeline_scheduler_snapshot.v1".into(),
            capacity: self.capacity,
            last_time_ms: self.last_time_ms,
            active: self
                .active
                .values()
                .map(|task| PlayerTimelineTaskSnapshot {
                    task_id: task.task_id.clone(),
                    target: task.target.clone(),
                    fence: task.fence.clone(),
                    deadline_ms: task.deadline_ms,
                })
                .collect(),
            completed: self.completed.iter().cloned().collect(),
        }
    }

    pub fn restore(snapshot: PlayerTimelineSchedulerSnapshot) -> Result<Self, PlayerTimelineError> {
        if snapshot.schema != "astra.player_timeline_scheduler_snapshot.v1"
            || snapshot.capacity == 0
            || snapshot.active.len() > snapshot.capacity
        {
            return Err(PlayerTimelineError::new(
                "ASTRA_PLAYER_TIMELINE_SNAPSHOT_INVALID",
                "timeline scheduler snapshot schema or capacity is invalid",
            ));
        }
        let mut active = BTreeMap::new();
        for task in snapshot.active {
            validate_symbol(&task.task_id, "task id")?;
            if task.target.trim().is_empty()
                || active
                    .insert(
                        task.task_id.clone(),
                        ActiveTimelineTask {
                            task_id: task.task_id,
                            target: task.target,
                            fence: task.fence,
                            deadline_ms: task.deadline_ms,
                        },
                    )
                    .is_some()
            {
                return Err(PlayerTimelineError::new(
                    "ASTRA_PLAYER_TIMELINE_SNAPSHOT_INVALID",
                    "timeline scheduler snapshot contains an invalid or duplicate task",
                ));
            }
        }
        let completed = snapshot
            .completed
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if completed
            .iter()
            .any(|task| validate_symbol(task, "completed task id").is_err())
            || completed.iter().any(|task| active.contains_key(task))
        {
            return Err(PlayerTimelineError::new(
                "ASTRA_PLAYER_TIMELINE_SNAPSHOT_INVALID",
                "timeline scheduler snapshot has invalid completed task state",
            ));
        }
        Ok(Self {
            capacity: snapshot.capacity,
            last_time_ms: snapshot.last_time_ms,
            active,
            completed,
        })
    }

    pub fn schedule(
        &mut self,
        task: PlayerTimelineTask,
        now_ms: u64,
    ) -> Result<Vec<PlayerTimelineCompletion>, PlayerTimelineError> {
        self.validate_time(now_ms)?;
        validate_symbol(&task.task_id, "task id")?;
        if task.schema != "astra.player_timeline_task.v1" {
            return Err(PlayerTimelineError::new(
                "ASTRA_PLAYER_TIMELINE_SCHEMA",
                "timeline task schema is unsupported",
            ));
        }
        match task.action {
            PlayerTimelineTaskAction::Start => {
                if self.active.contains_key(&task.task_id) {
                    return Err(PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_TASK_DUPLICATE",
                        "timeline task id is already active",
                    ));
                }
                if self.active.len() >= self.capacity {
                    return Err(PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_CAPACITY",
                        "timeline scheduler capacity is exhausted",
                    ));
                }
                self.completed.remove(&task.task_id);
                let target = task.target.ok_or_else(|| {
                    PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_TARGET_REQUIRED",
                        "timeline start requires a target",
                    )
                })?;
                validate_symbol(&target, "target")?;
                let duration_ms = task.duration_ms.filter(|value| *value > 0).ok_or_else(|| {
                    PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_DURATION_REQUIRED",
                        "timeline start requires a positive duration",
                    )
                })?;
                let deadline_ms = now_ms.checked_add(duration_ms).ok_or_else(|| {
                    PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_DEADLINE_OVERFLOW",
                        "timeline deadline overflowed",
                    )
                })?;
                self.active.insert(
                    task.task_id.clone(),
                    ActiveTimelineTask {
                        task_id: task.task_id,
                        target,
                        fence: task.fence,
                        deadline_ms,
                    },
                );
                self.last_time_ms = Some(now_ms);
                Ok(Vec::new())
            }
            PlayerTimelineTaskAction::Cancel => {
                let Some(active) = self.active.remove(&task.task_id) else {
                    if self.completed.remove(&task.task_id) {
                        self.last_time_ms = Some(now_ms);
                        return Ok(Vec::new());
                    }
                    return Err(PlayerTimelineError::new(
                        "ASTRA_PLAYER_TIMELINE_TASK_MISSING",
                        "timeline cancel references an unknown task",
                    ));
                };
                self.last_time_ms = Some(now_ms);
                Ok(vec![completion(
                    active,
                    PlayerTimelineCompletionKind::Cancelled,
                    now_ms,
                )])
            }
        }
    }

    pub fn poll(
        &mut self,
        now_ms: u64,
    ) -> Result<Vec<PlayerTimelineCompletion>, PlayerTimelineError> {
        self.validate_time(now_ms)?;
        let completed_ids = self
            .active
            .iter()
            .filter(|(_, task)| task.deadline_ms <= now_ms)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut completed = Vec::with_capacity(completed_ids.len());
        for id in completed_ids {
            let task = self
                .active
                .remove(&id)
                .expect("collected active timeline task must remain present");
            completed.push(completion(
                task,
                PlayerTimelineCompletionKind::Completed,
                now_ms,
            ));
            self.completed.insert(id);
        }
        self.last_time_ms = Some(now_ms);
        Ok(completed)
    }

    fn validate_time(&self, now_ms: u64) -> Result<(), PlayerTimelineError> {
        if self.last_time_ms.is_some_and(|last| now_ms < last) {
            return Err(PlayerTimelineError::new(
                "ASTRA_PLAYER_TIMELINE_CLOCK_REGRESSION",
                "timeline clock moved backwards",
            ));
        }
        Ok(())
    }
}

fn completion(
    task: ActiveTimelineTask,
    kind: PlayerTimelineCompletionKind,
    completed_at_ms: u64,
) -> PlayerTimelineCompletion {
    PlayerTimelineCompletion {
        schema: "astra.player_timeline_completion.v1".to_string(),
        task_id: task.task_id,
        target: task.target,
        fence: task.fence,
        kind,
        completed_at_ms,
    }
}

fn validate_symbol(value: &str, field: &str) -> Result<(), PlayerTimelineError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(PlayerTimelineError::new(
            "ASTRA_PLAYER_TIMELINE_SYMBOL_INVALID",
            format!("timeline {field} is not a safe symbol"),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTimelineError {
    pub code: &'static str,
    pub message: String,
}

impl PlayerTimelineError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PlayerTimelineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PlayerTimelineError {}
