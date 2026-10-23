use super::*;

impl AstraEmuManagerController {
    pub(super) fn session_running(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.closing.is_none())
    }

    pub(super) fn ensure_owner_idle(&self) -> Result<(), String> {
        match &self.active {
            Some(active) if active.closing.is_some() => Err("ASTRA_EMU_SESSION_CLOSING".into()),
            Some(_) => Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into()),
            None => Ok(()),
        }
    }

    pub(super) fn close_active(&mut self, reason: PlaySessionEndReason) -> Result<(), String> {
        if let Some(active) = self.active.as_mut() {
            if active.closing.is_none() {
                self.registry
                    .close(active.epoch, std::mem::take(&mut self.pending_events))?;
                active.closing = Some(reason);
            }
        }
        self.physical_keys.clear();
        self.held_controls.clear();
        self.key_modifiers = KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        };
        Ok(())
    }

    pub(super) fn flush_owner_events(&mut self) -> Result<(), String> {
        if self.pending_events.is_empty() {
            return Ok(());
        }
        if let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.closing.is_none())
        {
            self.registry
                .events(active.epoch, std::mem::take(&mut self.pending_events))?;
        }
        Ok(())
    }

    fn poll_owner(&mut self) -> Result<bool, String> {
        let Some(epoch) = self.active.as_ref().map(|active| active.epoch) else {
            return Ok(false);
        };
        let Some(update) = self.registry.take_update(epoch) else {
            return Ok(false);
        };
        if let Some(error) = update.action_error {
            self.action_message = astra_emu_manager_ui_slint::describe_error(&error).title;
            self.action_serial = self.action_serial.saturating_add(1);
        }
        let Some(active) = self.active.as_mut() else {
            return Ok(false);
        };
        active.failure = update.failure;
        let mut start_error = None;
        if update.opened && !active.opened {
            active.opened = true;
            match self
                .library
                .start_play_session(&active.game_id, unix_time_ms()?)
            {
                Ok(id) => self.active_play_session = Some(id),
                Err(error) => start_error = Some(error.to_string()),
            }
            if let (None, Some(surface)) = (&start_error, update.surface) {
                let game_id = active.game_id.clone();
                if let Err(error) = self.record_last_surface(&game_id, surface) {
                    self.diagnostic = error;
                    self.diagnostic_severity = "warning";
                }
            }
        }
        if !update.closed {
            if let Some(error) = start_error {
                self.close_active(PlaySessionEndReason::Crash)?;
                return Err(error);
            }
            return Ok(true);
        }
        let active = self.active.take().expect("active view checked above");
        self.pending_events.clear();
        let clear_error = self.mailbox.clear(active.epoch).err();
        let play_error = self
            .finish_play_session(active.closing.unwrap_or(if update.error.is_some() {
                PlaySessionEndReason::Crash
            } else {
                PlaySessionEndReason::Leave
            }))
            .err();
        let errors: Vec<_> = [start_error, update.error, clear_error, play_error]
            .into_iter()
            .flatten()
            .collect();
        if errors.is_empty() {
            Ok(true)
        } else {
            Err(errors.join("; "))
        }
    }

    pub(super) fn finish_play_session(
        &mut self,
        reason: PlaySessionEndReason,
    ) -> Result<(), String> {
        if let Some(id) = self.active_play_session.take() {
            self.library
                .end_play_session(&id, unix_time_ms()?, reason)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        let background_changed = self.poll_background()?;
        let patch_changed = self.poll_file_patch()?;
        let owner_changed = self.poll_owner()?;
        let connection_changed = self.poll_connection_test();
        let metadata_changed = self.poll_metadata()?;
        if background_changed
            || patch_changed
            || owner_changed
            || metadata_changed
            || connection_changed
        {
            Ok(Some(self.model()?))
        } else {
            Ok(None)
        }
    }

    pub(super) fn set_host_wake(&mut self, wake: HostWake) {
        self.registry.set_wake(wake.clone());
        self.host_wake = Some(wake.clone());
        self.metadata.set_wake(wake);
    }

    pub(super) fn runtime_deadline(&self) -> Option<Instant> {
        // Presentation/input servicing only. Native deadlines are owned by FamilyOwner.
        self.active
            .as_ref()
            .map(|_| Instant::now() + Duration::from_millis(8))
    }

    pub(super) fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String> {
        self.flush_owner_events()?;
        if self.poll_owner()? {
            Ok(Some(self.model()?))
        } else {
            Ok(None)
        }
    }
}
