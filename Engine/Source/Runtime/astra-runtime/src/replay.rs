use astra_core::Hash128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{RuntimeSnapshot, TickIntegrityMode, TickMode, TickReport, TickRequest};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeReplayTranscript {
    pub schema: String,
    pub checkpoint: RuntimeSnapshot,
    pub ticks: Vec<ReplayTick>,
}

#[derive(Debug)]
pub struct RuntimeReplayRecorder {
    checkpoint: RuntimeSnapshot,
    ticks: Vec<ReplayTick>,
}

impl RuntimeReplayRecorder {
    pub fn start(checkpoint: RuntimeSnapshot) -> Result<Self, crate::RuntimeError> {
        if checkpoint.integrity_mode != TickIntegrityMode::Evidence {
            return Err(crate::RuntimeError::diagnostic(
                astra_core::Diagnostic::blocking(
                    "ASTRA_RUNTIME_REPLAY_RECORDING_DISABLED",
                    "runtime replay recording requires evidence integrity mode",
                ),
            ));
        }
        Ok(Self {
            checkpoint,
            ticks: Vec::new(),
        })
    }

    pub fn record(
        &mut self,
        mut request: TickRequest,
        report: &TickReport,
        checkpoint: ReplayHashCheckpoint,
    ) -> Result<(), crate::RuntimeError> {
        if report.integrity_mode != TickIntegrityMode::Evidence {
            return Err(crate::RuntimeError::diagnostic(
                astra_core::Diagnostic::blocking(
                    "ASTRA_RUNTIME_REPLAY_RECORDING_DISABLED",
                    "runtime tick report was produced without evidence integrity hashes",
                ),
            ));
        }
        if request.timing.fixed_step != report.step {
            return Err(crate::RuntimeError::diagnostic(
                astra_core::Diagnostic::blocking(
                    "ASTRA_RUNTIME_REPLAY_RECORD_STEP",
                    "runtime replay request and tick report step do not match",
                ),
            ));
        }
        request.mode = TickMode::Replay;
        self.ticks.push(ReplayTick {
            request,
            expected: checkpoint,
        });
        Ok(())
    }

    pub fn finish(self) -> RuntimeReplayTranscript {
        RuntimeReplayTranscript {
            schema: "astra.runtime_replay_transcript.v3".to_string(),
            checkpoint: self.checkpoint,
            ticks: self.ticks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayTick {
    pub request: TickRequest,
    pub expected: ReplayHashCheckpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayHashCheckpoint {
    pub step: u64,
    pub state_hash: Hash128,
    pub event_hash: Hash128,
    pub presentation_hash: Hash128,
}
