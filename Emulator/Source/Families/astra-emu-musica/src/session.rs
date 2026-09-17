use crate::{
    audio::Audio,
    parse_sc,
    provider::SessionLease,
    scene::{error, read_asset, Scene},
    storage::{Snapshot, Storage},
    MusicaMountedVfs, MusicaVm, MusicaVmEvent, MusicaWaitState, ScOpcodeCatalog,
};
use astra_core::Hash256;
use astra_emu_family_api::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct PendingText {
    id: String,
    started: Instant,
}
pub(crate) struct MusicaSession {
    id: String,
    info: FrameInfo,
    archive: Arc<MusicaMountedVfs>,
    vm: MusicaVm,
    scene: Scene,
    audio: Audio,
    replacement: Option<TextReplacementServiceBox>,
    pending: Option<PendingText>,
    generation: u64,
    storage: Storage,
    game: Hash256,
    _lease: SessionLease,
    message: Option<(String, Option<String>)>,
    phase: u128,
    wait_ns: u64,
    input_pending: bool,
    control_keys: u8,
    pointer: Option<(f32, f32)>,
    finished: bool,
    poisoned: bool,
    suspended: bool,
}
impl MusicaSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        info: FrameInfo,
        archive: Arc<MusicaMountedVfs>,
        vm: MusicaVm,
        scene: Scene,
        audio: Audio,
        replacement: Option<TextReplacementServiceBox>,
        storage: Storage,
        game: Hash256,
        lease: SessionLease,
    ) -> Self {
        Self {
            id,
            info,
            archive,
            vm,
            scene,
            audio,
            replacement,
            storage,
            game,
            _lease: lease,
            pending: None,
            generation: 0,
            message: None,
            phase: 0,
            wait_ns: 0,
            input_pending: false,
            control_keys: 0,
            pointer: None,
            finished: false,
            poisoned: false,
            suspended: false,
        }
    }
    fn cancel_text(&mut self) -> FamilyResult<()> {
        if let Some(pending) = self.pending.take() {
            if let Some(service) = &self.replacement {
                service.cancel(pending.id.into()).into_result()?;
            }
        }
        Ok(())
    }
    fn poll_text(&mut self) -> FamilyResult<bool> {
        let Some(pending) = &self.pending else {
            return Ok(false);
        };
        let service = self
            .replacement
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_STATE", "text service is unavailable"))?;
        if pending.started.elapsed() > Duration::from_secs(15) {
            self.cancel_text()?;
            tracing::warn!(event = "astra.emu.musica.translation.timeout");
            return Ok(false);
        }
        match service.poll(pending.id.clone().into()).into_result() {
            Ok(TextPollResult::Pending) => Ok(false),
            Ok(TextPollResult::Ready(response)) => {
                let valid = response.validate().is_ok() && response.request_id == pending.id;
                self.pending = None;
                if !valid {
                    tracing::warn!(event = "astra.emu.musica.translation.invalid");
                    return Ok(false);
                }
                if let Some((_, speaker)) = &self.message {
                    let replacement = (response.replacement.to_string(), speaker.clone());
                    if self
                        .scene
                        .render(self.vm.state(), Some(&replacement), None)
                        .is_ok()
                    {
                        self.message = Some(replacement);
                    } else {
                        tracing::warn!(event = "astra.emu.musica.translation.render_failed");
                    }
                }
                Ok(false)
            }
            Ok(TextPollResult::Cancelled | TextPollResult::Failed(_)) | Err(_) => {
                self.pending = None;
                tracing::warn!(event = "astra.emu.musica.translation.failed");
                Ok(false)
            }
        }
    }
    fn message(&mut self, text: String, speaker: Option<String>) -> FamilyResult<()> {
        self.cancel_text()?;
        if text.len() > MAX_TEXT_BYTES
            || speaker.as_ref().is_some_and(|s| s.len() > MAX_SYMBOL_BYTES)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_BOUND",
                "message exceeds supported bounds",
            ));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_SEQUENCE", "text sequence overflowed"))?;
        self.message = Some((text.clone(), speaker.clone()));
        if let Some(service) = &self.replacement {
            let id = format!("{}.text.{}", self.id, self.generation);
            let request = TextReplacementRequest {
                request_id: id.clone().into(),
                source: text.into(),
                speaker: speaker.unwrap_or_default().into(),
                ruby: "".into(),
            };
            match service.submit(request).into_result() {
                Ok(()) => {
                    self.pending = Some(PendingText {
                        id,
                        started: Instant::now(),
                    })
                }
                Err(_) => tracing::warn!(event = "astra.emu.musica.translation.submit_failed"),
            }
        }
        Ok(())
    }
    fn save(&self) -> FamilyResult<()> {
        let vm = self
            .vm
            .encode_native_save()
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        self.storage.write(&Snapshot {
            game: self.game,
            vm,
            message: self.message.clone(),
            wait_ns: self.wait_ns,
            sounds: self.audio.snapshot()?,
        })?;
        tracing::info!(event = "astra.emu.musica.save.completed");
        Ok(())
    }
    fn load(&mut self) -> FamilyResult<()> {
        let saved = self.storage.read()?;
        if saved.game != self.game {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_GAME",
                "save belongs to another archive/profile identity",
            ));
        }
        let state = MusicaVm::decode_native_save(&saved.vm)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM save is invalid"))?;
        let bytes = read_asset(&self.archive, &state.script_uri, 16 * 1024 * 1024)?;
        if Hash256::from_sha256(&bytes) != state.script_hash {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_SCRIPT",
                "saved script has changed",
            ));
        }
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_musica())
            .map_err(|_| error("ASTRA_EMU_MUSICA_SCRIPT", "saved script cannot be parsed"))?;
        let mut vm = MusicaVm::new(
            state.script_uri,
            state.script_hash,
            script,
            state.session_seed,
        )
        .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved script is invalid"))?;
        vm.restore_native_save(&saved.vm, 1)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved VM state is invalid"))?;
        let mut scene = Scene::new(self.archive.clone(), self.info.width, self.info.height)?;
        let choices = vm.choice_display().map_err(vm_error)?;
        scene.render(
            vm.state(),
            saved.message.as_ref(),
            choices
                .as_ref()
                .map(|(labels, index)| (labels.as_slice(), *index)),
        )?;
        self.cancel_text()?;
        if let Some(service) = &self.replacement {
            service.reset(TextResetReason::Load).into_result()?;
        }
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        self.audio.restore(saved.sounds)?;
        vm.set_control_pressed(self.control_keys != 0);
        self.vm = vm;
        self.scene = scene;
        self.message = saved.message;
        self.wait_ns = saved.wait_ns;
        self.phase = 0;
        self.input_pending = false;
        self.pointer = None;
        self.finished = self.vm.state().terminal;
        tracing::info!(event = "astra.emu.musica.load.completed");
        Ok(())
    }
    fn tick(&mut self) -> FamilyResult<bool> {
        let tick = self
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TICK", "tick counter overflowed"))?;
        if let Some(wait) = self.vm.state().wait.clone() {
            let delta = ((u128::from(tick) * 1_000_000_000 / 60)
                - (u128::from(tick - 1) * 1_000_000_000 / 60)) as u64;
            self.wait_ns = self.wait_ns.saturating_add(delta);
            let (id, ready) = match wait {
                MusicaWaitState::Input { token_id } => (
                    token_id,
                    (self.input_pending
                        || (self.vm.state().choice.is_none()
                            && self.vm.control_fast_forward_active()
                            && self.wait_ns >= 10_000_000))
                        && self.pending.is_none(),
                ),
                MusicaWaitState::Time {
                    token_id,
                    milliseconds,
                    ..
                } => (
                    token_id,
                    self.vm.control_fast_forward_active()
                        || self.wait_ns >= u64::from(milliseconds) * 1_000_000,
                ),
                MusicaWaitState::Presentation { token_id, .. } => (token_id, true),
                MusicaWaitState::Media { .. } | MusicaWaitState::Provider { .. } => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_WAIT_UNSUPPORTED",
                        "script requires an unimplemented media or provider wait",
                    ))
                }
            };
            if ready {
                if self.vm.state().choice.is_some() {
                    self.vm.commit_choice().map_err(vm_error)?;
                } else {
                    self.vm.resolve_wait(&id).map_err(vm_error)?;
                }
                self.wait_ns = 0;
                self.input_pending = false;
            } else {
                self.vm.advance_waiting_tick(tick).map_err(vm_error)?;
                return Ok(false);
            }
        }
        let event = self.vm.step(tick).map_err(vm_error)?;
        match event {
            Some(MusicaVmEvent::Choice) => {
                self.message = None;
                Ok(true)
            }
            Some(MusicaVmEvent::Message { text, speaker, .. }) => {
                self.wait_ns = 0;
                self.message(text, speaker)?;
                Ok(true)
            }
            Some(MusicaVmEvent::Wait(_)) => {
                self.wait_ns = 0;
                Ok(false)
            }
            Some(MusicaVmEvent::Audio { commands }) => {
                self.audio.apply(commands)?;
                Ok(false)
            }
            Some(MusicaVmEvent::Chain { target }) => {
                let (file, label) =
                    crate::script::chain_target_parts(&target).ok_or_else(|| {
                        error(
                            "ASTRA_EMU_MUSICA_RUNTIME_CHAIN",
                            "invalid chained script location",
                        )
                    })?;
                let uri = format!("musica:/scr/{file}");
                let bytes = read_asset(&self.archive, &uri, 16 * 1024 * 1024)?;
                let script =
                    parse_sc(&bytes, &ScOpcodeCatalog::observed_musica()).map_err(|_| {
                        error("ASTRA_EMU_MUSICA_SCRIPT", "chained script cannot be parsed")
                    })?;
                self.vm
                    .replace_script(uri, Hash256::from_sha256(&bytes), script, label)
                    .map_err(vm_error)?;
                Ok(false)
            }
            Some(MusicaVmEvent::Stage(_)) => Ok(true),
            Some(
                MusicaVmEvent::Effect(_)
                | MusicaVmEvent::EffectCleared
                | MusicaVmEvent::Panel { .. },
            ) => Ok(true),
            Some(MusicaVmEvent::Terminal) => {
                self.finished = true;
                Ok(false)
            }
            None => Ok(false),
        }
    }
    fn advance_inner(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        AdvanceRequest {
            session_id: self.id.clone().into(),
            elapsed_ns,
            events: events.to_vec().into(),
        }
        .validate()?;
        if elapsed_ns > 1_000_000_000 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ELAPSED",
                "elapsed interval exceeds the bounded catch-up window",
            ));
        }
        self.audio.check()?;
        let mut save = false;
        let mut load = false;
        let mut choice_dirty = false;
        for event in events {
            match event {
                FamilyEvent::PointerMove { x, y } => {
                    self.pointer = Some((*x, *y));
                    if let Some((labels, _)) = self.vm.choice_display().map_err(vm_error)? {
                        if let Some(index) = crate::text_renderer::choice_at(labels.len(), *x, *y) {
                            choice_dirty |= self.vm.focus_choice(index).map_err(vm_error)?;
                        }
                    }
                }
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } if self.vm.state().choice.is_some() => {
                    let (labels, _) =
                        self.vm.choice_display().map_err(vm_error)?.ok_or_else(|| {
                            error("ASTRA_EMU_MUSICA_CHOICE", "choice is unavailable")
                        })?;
                    if let Some(index) = self
                        .pointer
                        .and_then(|(x, y)| crate::text_renderer::choice_at(labels.len(), x, y))
                    {
                        choice_dirty |= self.vm.focus_choice(index).map_err(vm_error)?;
                        self.input_pending = true;
                    }
                }
                FamilyEvent::Key {
                    code: KeyCode::ArrowUp | KeyCode::ArrowDown,
                    state: KeyState::Pressed,
                    ..
                } if self.vm.state().choice.is_some() => {
                    let direction = if matches!(
                        event,
                        FamilyEvent::Key {
                            code: KeyCode::ArrowUp,
                            ..
                        }
                    ) {
                        -1
                    } else {
                        1
                    };
                    self.vm.move_choice(direction).map_err(vm_error)?;
                    choice_dirty = true;
                }
                FamilyEvent::Key {
                    code: KeyCode::F5,
                    state: KeyState::Pressed,
                    ..
                } => save = true,
                FamilyEvent::Key {
                    code: KeyCode::F9,
                    state: KeyState::Pressed,
                    ..
                } => load = true,
                FamilyEvent::Key {
                    code: KeyCode::Enter | KeyCode::Space,
                    state: KeyState::Pressed,
                    ..
                }
                | FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } => {
                    if matches!(self.vm.state().wait, Some(MusicaWaitState::Input { .. }))
                        && self.pending.is_none()
                    {
                        self.input_pending = true;
                    }
                }
                FamilyEvent::Key {
                    code: KeyCode::ControlLeft | KeyCode::ControlRight,
                    state,
                    ..
                } => {
                    let mask = if matches!(
                        event,
                        FamilyEvent::Key {
                            code: KeyCode::ControlLeft,
                            ..
                        }
                    ) {
                        1
                    } else {
                        2
                    };
                    if *state == KeyState::Pressed {
                        self.control_keys |= mask;
                    } else {
                        self.control_keys &= !mask;
                    }
                    self.vm.set_control_pressed(self.control_keys != 0);
                }
                FamilyEvent::WindowFocused { focused: false } => {
                    self.control_keys = 0;
                    self.vm.set_control_pressed(false);
                }
                FamilyEvent::WindowSuspended { suspended } => {
                    if *suspended {
                        self.control_keys = 0;
                        self.vm.set_control_pressed(false);
                    }
                    self.suspended = *suspended;
                    self.audio.suspend(*suspended)?;
                }
                FamilyEvent::WindowCloseRequested => {
                    self.finished = true;
                }
                _ => {}
            }
        }
        if save && load {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_CONFLICT",
                "save and load cannot be requested together",
            ));
        }
        if load {
            self.load()?;
        }
        let mut dirty = self.poll_text()? || choice_dirty;
        if !self.suspended && !self.finished {
            dirty |= self
                .vm
                .advance_effect_clock(elapsed_ns)
                .map_err(vm_error)?
                .is_some();
            self.phase += u128::from(elapsed_ns) * 60;
            while self.phase >= 1_000_000_000 && !self.finished {
                self.phase -= 1_000_000_000;
                dirty |= self.tick()?;
            }
        }
        if dirty {
            let choices = self.vm.choice_display().map_err(vm_error)?;
            self.scene.render(
                self.vm.state(),
                self.message.as_ref(),
                choices
                    .as_ref()
                    .map(|(labels, index)| (labels.as_slice(), *index)),
            )?;
        }
        if save {
            self.save()?;
        }
        Ok(AdvanceResponse {
            status: if self.finished {
                FamilyStatus::Finished
            } else if self.vm.state().wait.is_some() {
                FamilyStatus::Waiting
            } else {
                FamilyStatus::Running
            },
        })
    }
}
impl FamilySession for MusicaSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        if self.poisoned {
            return Err(error(
                "ASTRA_EMU_MUSICA_POISONED",
                "failed session must be closed",
            ));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.advance_inner(elapsed_ns, events)
        }))
        .unwrap_or_else(|_| {
            Err(error(
                "ASTRA_EMU_MUSICA_SESSION_PANIC",
                "session panicked and must be closed",
            ))
        });
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        visitor.accept(FrameView::from_slice(&self.scene.pixels, self.info)?)
    }
    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        let text = self.cancel_text();
        let audio = self.audio.shutdown();
        tracing::info!(event = "astra.emu.musica.session.close");
        text?;
        audio
    }
}
impl Drop for MusicaSession {
    fn drop(&mut self) {
        let _ = self.cancel_text();
        let _ = self.audio.shutdown();
    }
}
fn vm_error(cause: crate::MusicaRuntimeError) -> FamilyError {
    if let crate::MusicaRuntimeError::UnsupportedOpcode { ordinal, .. } = &cause {
        return error(
            cause.diagnostic_code(),
            &format!("script command at ordinal {ordinal} is not implemented"),
        );
    }
    error(
        cause.diagnostic_code(),
        "script execution failed at an unsupported or invalid operation",
    )
}

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
