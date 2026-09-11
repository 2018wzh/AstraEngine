use super::*;

impl AstraEmuManagerController {
    pub(super) fn close_active(&mut self, reason: PlaySessionEndReason) -> Result<(), String> {
        self.pending_events.clear();
        self.physical_keys.clear();
        self.held_controls.clear();
        self.key_modifiers = KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        };
        let close_error = self
            .active
            .take()
            .and_then(|mut active| active.close().err());
        let play_error = self
            .active_play_session
            .take()
            .map(|session_id| {
                self.library
                    .end_play_session(&session_id, unix_time_ms()?, reason)
                    .map_err(|error| error.to_string())
            })
            .transpose()
            .err();
        match (close_error, play_error) {
            (Some(first), Some(second)) => Err(format!("{first}; {second}")),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (None, None) => Ok(()),
        }
    }

    pub(super) fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        if let Some(active) = self.active.as_ref() {
            if let Some(audio) = active.audio.as_ref() {
                audio.check_health()?;
            }
        }
        let connection_changed = self.poll_connection_test();
        if self.poll_metadata()? || connection_changed {
            Ok(Some(self.model()?))
        } else {
            Ok(None)
        }
    }

    pub(super) fn set_host_wake(&mut self, wake: HostWake) {
        self.host_wake = Some(wake.clone());
        self.metadata.set_wake(wake);
    }

    pub(super) fn runtime_deadline(&self) -> Option<Instant> {
        self.active.as_ref().map(|active| active.next_deadline)
    }

    pub(super) fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String> {
        let Some(active) = self.active.as_mut() else {
            return Ok(None);
        };
        let elapsed = active
            .last_tick
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        let events = std::mem::take(&mut self.pending_events);
        active.advance(elapsed.max(1), &events)?;
        if active.status == FamilyStatus::Finished {
            self.close_active(PlaySessionEndReason::Leave)?;
            return Ok(Some(self.model()?));
        }
        Ok(None)
    }
}
