use super::*;
use crate::{NativeVnProductMediaHost, NativeVnProductMediaSnapshot};
use astra_vn_core::{
    PreviewCheckpointInfo, PreviewIdentity, PreviewRejectCode as Reject, PreviewStatus,
};

use super::product_save::NativeVnRestoreState;
struct Checkpoint {
    info: PreviewCheckpointInfo,
    state: NativeVnRestoreState,
    media: NativeVnProductMediaSnapshot,
    bytes: usize,
}
/// One editor-owned preview of the existing Player. It never runs a second VN VM.
pub struct NativeVnPreview {
    identity: PreviewIdentity,
    source_scope: astra_runtime::TaskScope,
    sequence: u64,
    paused: bool,
    current_time_ns: u64,
    source_id: Option<String>,
    checkpoints: std::collections::VecDeque<Checkpoint>,
    next_checkpoint: u64,
    fragment_step: u64,
    retained_bytes: usize,
}
impl NativeVnPreview {
    pub fn attach(
        source: &NativeVnHostCommandSource,
        identity: PreviewIdentity,
    ) -> Result<Self, Reject> {
        if identity.project_hash != source.compiled_project_hash || identity.generation == 0 {
            return Err(Reject::StaleIdentity);
        }
        if serde_json::to_vec(&identity)
            .map_err(|_| Reject::InvalidRequest)?
            .len()
            > 32_768
        {
            return Err(Reject::InvalidRequest);
        }
        if identity.documents.is_empty()
            || identity.documents.len() > 256
            || identity.documents.iter().any(|(path, revision)| {
                revision.version == 0
                    || path.is_empty()
                    || path.starts_with('/')
                    || path.contains('\\')
                    || path.contains(':')
                    || path
                        .split('/')
                        .any(|part| part.is_empty() || part == ".." || part == ".")
            })
        {
            return Err(Reject::InvalidRequest);
        }
        Ok(Self {
            identity,
            source_scope: source.media_scope.child(),
            sequence: 0,
            paused: false,
            current_time_ns: source.stage_director.state().elapsed_ns,
            source_id: None,
            checkpoints: Default::default(),
            next_checkpoint: 1,
            fragment_step: source.fixed_step,
            retained_bytes: 0,
        })
    }
    pub fn accept(&mut self, identity: &PreviewIdentity, sequence: u64) -> Result<(), Reject> {
        if identity != &self.identity || self.source_scope.is_cancelled() {
            return Err(Reject::StaleIdentity);
        }
        if sequence <= self.sequence {
            return Err(Reject::StaleRequest);
        }
        self.sequence = sequence;
        Ok(())
    }
    pub fn is_cancelled(&self) -> bool {
        self.source_scope.is_cancelled()
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn status(&self) -> PreviewStatus {
        PreviewStatus {
            identity: self.identity.clone(),
            paused: self.paused,
            presentation_time_ns: self.current_time_ns,
            source_id: self.source_id.clone(),
            checkpoints: self.checkpoints.iter().map(|c| c.info.clone()).collect(),
        }
    }
    pub async fn set_paused(
        &mut self,
        paused: bool,
        media: &mut NativeVnProductMediaHost,
        executor: &astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<(), Reject> {
        media
            .set_output_paused(paused, executor)
            .await
            .map_err(|_| Reject::NotRecoverable)?;
        self.paused = paused;
        Ok(())
    }
    /// Called at a completed fixed-tick boundary, after media and story completions.
    /// At most 64 checkpoints / 32 MiB are retained, only for the current source fragment.
    pub fn record(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        media: &NativeVnProductMediaHost,
    ) -> Result<bool, Reject> {
        if self.source_scope.is_cancelled() {
            return Err(Reject::StaleIdentity);
        }
        self.current_time_ns = source.stage_director.state().elapsed_ns;
        let id = source
            .pending_wait()
            .map(|wait| wait.command_id.clone())
            .filter(|id| source.story.source_map.contains_key(id));
        let changed = id != self.source_id || self.fragment_step != source.fixed_step;
        if changed {
            self.source_id = id;
            self.fragment_step = source.fixed_step;
            self.checkpoints.clear();
            self.retained_bytes = 0;
        }
        if self.source_id.is_none() || self.paused {
            return Ok(changed);
        }
        let time = source.stage_director.state().elapsed_ns;
        if self
            .checkpoints
            .back()
            .is_some_and(|c| time.saturating_sub(c.info.presentation_time_ns) < 100_000_000)
        {
            return Ok(changed);
        }
        source
            .ensure_presentation_active()
            .map_err(|_| Reject::NotRecoverable)?;
        let state = NativeVnRestoreState {
            runtime: source.host.save().map_err(|_| Reject::NotRecoverable)?,
            stage_director: source.stage_director.clone(),
            director_transition_snapshot: save_director_transition_snapshot(
                source.director_transition_snapshot.as_ref(),
            )
            .map_err(|_| Reject::NotRecoverable)?,
            step_evidence: source
                .last_step_evidence
                .clone()
                .ok_or(Reject::NotRecoverable)?,
        };
        let media = media.snapshot();
        state
            .stage_director
            .snapshot()
            .map_err(|_| Reject::NotRecoverable)?;
        let bytes = postcard::to_allocvec(&state)
            .map_err(|_| Reject::NotRecoverable)?
            .len()
            + serde_json::to_vec(&media)
                .map_err(|_| Reject::NotRecoverable)?
                .len();
        if bytes > 32 * 1024 * 1024 {
            return Err(Reject::NotRecoverable);
        }
        while self.checkpoints.len() >= 64 || self.retained_bytes + bytes > 32 * 1024 * 1024 {
            self.retained_bytes -= self
                .checkpoints
                .pop_front()
                .expect("bounded checkpoint queue")
                .bytes;
        }
        let id = self.next_checkpoint;
        self.next_checkpoint = id.checked_add(1).ok_or(Reject::NotRecoverable)?;
        self.retained_bytes += bytes;
        self.checkpoints.push_back(Checkpoint {
            info: PreviewCheckpointInfo {
                id,
                presentation_time_ns: time,
            },
            state,
            media,
            bytes,
        });
        Ok(true)
    }
    pub async fn seek(
        &mut self,
        source_id: &str,
        checkpoint: u64,
        source: &mut NativeVnHostCommandSource,
        media: &mut NativeVnProductMediaHost,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<PlayerHostCommandBatch, Reject> {
        if !self.paused {
            return Err(Reject::NotPaused);
        }
        if self.source_scope.is_cancelled() {
            return Err(Reject::StaleIdentity);
        }
        if self.fragment_step != source.fixed_step
            || self.source_id.as_deref() != Some(source_id)
            || source.pending_wait().map(|w| w.command_id.as_str()) != Some(source_id)
        {
            return Err(Reject::CrossFragment);
        }
        let checkpoint = self
            .checkpoints
            .iter()
            .find(|c| c.info.id == checkpoint)
            .ok_or(Reject::CheckpointUnavailable)?;
        // Preview checkpoints are process-local and immutable. Validate every media
        // reference before touching World; restore never executes story commands or IO.
        media
            .validate_restore(&checkpoint.media)
            .map_err(|_| Reject::NotRecoverable)?;
        let mut committed = false;
        let result = source.restore_explicit_state(checkpoint.state.clone(), None, &mut committed);
        let batch = match result {
            Ok(batch) => batch,
            Err(_) => {
                if committed {
                    source.presentation_failed = true;
                    source.media_scope.cancel();
                }
                return Err(Reject::RestoreFailed);
            }
        };
        if media
            .restore_product_media(source, executor, checkpoint.media.clone())
            .await
            .is_err()
            || media.set_output_paused(true, executor).await.is_err()
        {
            source.presentation_failed = true;
            source.media_scope.cancel();
            return Err(Reject::RestoreFailed);
        }
        self.current_time_ns = checkpoint.info.presentation_time_ns;
        self.source_scope = source.media_scope.child();
        Ok(batch)
    }
    pub fn cancel(&mut self) {
        self.source_scope.cancel();
        self.checkpoints.clear();
        self.retained_bytes = 0;
    }
}
impl Drop for NativeVnPreview {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
