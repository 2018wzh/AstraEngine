use super::*;
use astra_emu_manager::window_service::WindowBridge;
use astra_emu_manager_core::model::FramePoll;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NativeFailure {
    pub(super) message: String,
    pub(super) can_retry: bool,
}
pub(super) struct SessionServices {
    pub(super) text: Option<TextReplacementBridge>,
    pub(super) modal: Option<ModalBridge>,
    pub(super) window: Option<WindowBridge>,
    pub(super) cancellation: astra_emu_manager_core::NativeCancellation,
}
pub(super) struct ActiveFamilySession {
    session_epoch: u64,
    session: Option<NativeSession>,
    text: Option<TextReplacementBridge>,
    modal: Option<ModalBridge>,
    window: Option<WindowBridge>,
    mailbox: FrameMailbox,
    pub(super) status: FamilyStatus,
    next_poll: Instant,
    pub(super) failure: Option<NativeFailure>,
}
impl ActiveFamilySession {
    pub(super) fn open(
        game_config: (
            &GameRecord,
            Vec<astra_emu_manager_core::model::ConfigEntry>,
            u64,
        ),
        candidate: &FamilyProbeCandidate,
        registry: &mut FamilyProviderRegistry,
        mailbox: FrameMailbox,
        initial_window: WindowState,
        services: SessionServices,
    ) -> Result<Self, String> {
        let (game, configuration, epoch) = game_config;
        if candidate.descriptor.family_id != candidate.report.family_id
            || game.family_id.as_deref() != Some(candidate.report.family_id.as_str())
        {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        mailbox.activate(epoch).map_err(|e| e.to_string())?;
        let mut host = astra_emu_manager_core::no_host_services(&services.cancellation);
        if let Some(text) = &services.text {
            host.text = text.sink();
        }
        if let Some(window) = &services.window {
            host.window = window.sink();
        }
        let opened = registry.open_selected(
            candidate,
            NativeOpenRequest {
                game_path: game.location.clone(),
                configuration,
                session_epoch: epoch,
                initial_window,
                host,
                cancellation: services.cancellation,
                d3d12: mailbox.d3d12()?,
            },
        );
        let session = match opened {
            Ok(session) => session,
            Err(error) => {
                if let Some(text) = &services.text {
                    let _ = text.close();
                }
                if let Some(window) = &services.window {
                    window.close();
                }
                if let Some(modal) = &services.modal {
                    modal.cancel_pending();
                }
                return Err(error.to_string());
            }
        };
        let mut active = Self {
            session_epoch: epoch,
            session: Some(session),
            text: services.text,
            modal: services.modal,
            window: services.window,
            mailbox,
            status: FamilyStatus::Running,
            next_poll: Instant::now(),
            failure: None,
        };
        if let Err(error) = active.poll() {
            let cleanup = active.close();
            return Err(cleanup
                .err()
                .map_or(error.clone(), |e| format!("{error}; {e}")));
        }
        Ok(active)
    }
    pub(super) fn epoch(&self) -> u64 {
        self.session_epoch
    }
    pub(super) fn wake_deadline(&self) -> Instant {
        self.next_poll
    }
    pub(super) fn input(&mut self, events: &[FamilyEvent]) -> Result<(), String> {
        if events.iter().any(|event| matches!(event, FamilyEvent::Control { command } if *command == astra_emu_abi::CONTROL_RETRY)) { self.failure = None; }
        let displayed = self.mailbox.displayed_identity()?;
        self.session
            .as_mut()
            .ok_or("ASTRA_EMU_CORE_CLOSED")?
            .input(events, displayed)
            .map_err(|e| e.to_string())
    }
    /// Host cadence only controls observation; native producer owns all game time.
    pub(super) fn poll(&mut self) -> Result<bool, String> {
        let session = self.session.as_mut().ok_or("ASTRA_EMU_CORE_CLOSED")?;
        let state = session.state().map_err(|e| e.to_string())?;
        let failure_changed = if state.state == astra_emu_abi::SESSION_FAILED {
            let can_retry = state.flags & astra_emu_abi::SESSION_CAN_RETRY != 0;
            let code = String::from_utf8_lossy(&state.error.code[..state.error.code_len as usize]);
            let message =
                String::from_utf8_lossy(&state.error.message[..state.error.message_len as usize]);
            let changed = self.failure.as_ref().is_none_or(|failure| {
                failure.can_retry != can_retry
                    || failure
                        .message
                        .strip_prefix(code.as_ref())
                        .and_then(|suffix| suffix.strip_prefix(": "))
                        != Some(message.as_ref())
            });
            if changed {
                tracing::error!(event = "astra.emu.native.session_failed", operation = "state", diagnostic_code = %code, message = %message, can_retry);
                self.failure = Some(NativeFailure {
                    message: format!("{code}: {message}"),
                    can_retry,
                });
            }
            self.next_poll = Instant::now() + Duration::from_millis(10);
            return Ok(changed);
        } else {
            self.failure.take().is_some()
        };
        self.status = match state.state {
            astra_emu_abi::SESSION_RUNNING => FamilyStatus::Running,
            astra_emu_abi::SESSION_PAUSED | astra_emu_abi::SESSION_WAITING => FamilyStatus::Waiting,
            astra_emu_abi::SESSION_FINISHED => FamilyStatus::Finished,
            _ => return Err("ASTRA_EMU_CORE_STATE".into()),
        };
        let after = self.mailbox.identity()?;
        let polled = session.acquire_frame(after).map_err(|e| e.to_string())?;
        let ready = matches!(&polled, FramePoll::Ready(_));
        if let FramePoll::Ready(frame) = polled {
            self.mailbox.publish(frame).map_err(|e| e.to_string())?;
        }
        self.next_poll = Instant::now() + Duration::from_millis(2);
        Ok(ready || failure_changed)
    }
    pub(super) fn close(&mut self) -> Result<(), String> {
        // Native cancellation is independent of ordinary controls and wakes all
        // synchronous services before worker joins. CPU leases outlive close.
        if let Some(session) = &self.session {
            session.cancel();
        }
        let text_result = self
            .text
            .as_ref()
            .map_or(Ok(()), |text| text.close().map_err(|e| e.to_string()));
        if let Some(window) = &self.window {
            window.close();
        }
        if let Some(modal) = &self.modal {
            modal.cancel_pending();
        }
        let frame_result = self.mailbox.clear(self.session_epoch);
        let core_result = self.session.take().map_or(Ok(()), |mut session| {
            session.close().map_err(|e| e.to_string())
        });
        self.text.take();
        self.window.take();
        self.modal.take();
        let errors: Vec<_> = [text_result, frame_result, core_result]
            .into_iter()
            .filter_map(Result::err)
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
impl Drop for ActiveFamilySession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "astra.emu.session.cleanup_failed", error);
        }
    }
}
