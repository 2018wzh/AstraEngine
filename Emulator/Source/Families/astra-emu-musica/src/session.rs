mod backlog;
mod config;
mod gallery;
mod lifecycle;
mod tick;
use lifecycle::vm_error;
mod message;
mod movie;
mod persistence;
mod save_pages;
mod title;
use crate::{
    audio::Audio,
    provider::SessionLease,
    scene::{error, Scene},
    storage::{Snapshot, Storage},
    MusicaMountedVfs, MusicaVm, MusicaVmEvent, MusicaWaitState,
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
    movie: Option<crate::movie::Movie>,
    replacement: Option<TextReplacementServiceBox>,
    pending: Option<PendingText>,
    voice_duration: Option<(std::sync::mpsc::Receiver<FamilyResult<u32>>, Instant)>,
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
    config_pointer_down: bool,
    pending_fullscreen: Option<bool>,
    reset_host_clock: bool,
    finished: bool,
    poisoned: bool,
    suspended: bool,
    focused: bool,
    visible: bool,
    progress_in_background: bool,
    primary_encoding: crate::ScriptEncoding,
    gameplay_frame: Option<Arc<[u8]>>,
    save_cards: Vec<(u32, crate::storage::SaveCard)>,
    quick_cursor: u32,
    persisted_unlocks: Vec<Hash256>,
    entry_uri: String,
    title_focus: Option<u32>,
    load_from_title: bool,
    last_quick_save_pc_line: Option<u32>,
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
        focused: bool,
        visible: bool,
        progress_in_background: bool,
        primary_encoding: crate::ScriptEncoding,
        quick_cursor: u32,
    ) -> Self {
        let persisted_unlocks = vm.state().gallery_unlocks.clone();
        let entry_uri = vm.state().script_uri.clone();
        let pending_fullscreen = vm.config().fullscreen.then_some(true);
        Self {
            persisted_unlocks,
            entry_uri,
            title_focus: None,
            load_from_title: false,
            id,
            info,
            archive,
            vm,
            scene,
            audio,
            movie: None,
            replacement,
            storage,
            game,
            _lease: lease,
            pending: None,
            voice_duration: None,
            generation: 0,
            message: None,
            phase: 0,
            wait_ns: 0,
            input_pending: false,
            control_keys: 0,
            pointer: None,
            config_pointer_down: false,
            pending_fullscreen,
            reset_host_clock: false,
            finished: false,
            poisoned: false,
            suspended: false,
            focused,
            visible,
            progress_in_background,
            primary_encoding,
            gameplay_frame: None,
            save_cards: Vec::new(),
            quick_cursor,
            last_quick_save_pc_line: None,
        }
    }

    fn paused(&self) -> bool {
        self.suspended || !self.visible || (!self.focused && !self.progress_in_background)
    }
    fn clear_input(&mut self) {
        self.control_keys = 0;
        self.input_pending = false;
        self.pointer = None;
        self.config_pointer_down = false;
        self.vm.set_control_pressed(false);
    }
    fn update_pause(&self) -> FamilyResult<()> {
        self.audio.suspend(self.paused())?;
        tracing::debug!(
            event = "astra.emu.musica.window.pause",
            focused = self.focused,
            suspended = self.suspended,
            paused = self.paused()
        );
        Ok(())
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
        let mut focused = self.focused;
        let mut visible = self.visible;
        let mut suspended = self.suspended;
        let mut was_paused = suspended
            || !visible
            || (!focused && !self.progress_in_background);
        let mut discard_elapsed = false;
        let mut resumed_from_pause = false;
        for event in events {
            match event {
                FamilyEvent::WindowFocused { focused: value } => {
                    focused = *value;
                }
                FamilyEvent::WindowVisibility { visible: value } => {
                    visible = *value;
                }
                FamilyEvent::WindowSuspended { suspended: value } => {
                    suspended = *value;
                }
                _ => {}
            }
            if matches!(
                event,
                FamilyEvent::WindowFocused { .. }
                    | FamilyEvent::WindowVisibility { .. }
                    | FamilyEvent::WindowSuspended { .. }
            ) {
                let paused = suspended
                    || !visible
                    || (!focused && !self.progress_in_background);
                if !was_paused && paused {
                    discard_elapsed = true;
                } else if was_paused && !paused {
                    resumed_from_pause = true;
                }
                was_paused = paused;
            }
        }
        let lifecycle_paused = suspended
            || !visible
            || (!focused && !self.progress_in_background);
        let elapsed_ns = if discard_elapsed || lifecycle_paused {
            0
        } else if resumed_from_pause {
            if elapsed_ns > 1_000_000_000 {
                0
            } else {
                elapsed_ns
            }
        } else {
            elapsed_ns
        };
        if elapsed_ns > 1_000_000_000 {
            tracing::warn!(
                event = "astra.emu.musica.elapsed_rejected",
                diagnostic_code = "ASTRA_EMU_MUSICA_ELAPSED",
                elapsed_ns,
                phase = self.phase,
                wait_ns = self.wait_ns,
                suspended = self.suspended,
                focused = self.focused,
                progress_in_background = self.progress_in_background
            );
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
            if self.config_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.gallery_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.title_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.save_page_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.backlog_event(event)? {
                choice_dirty = true;
                continue;
            }
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
                    code: KeyCode::A | KeyCode::S,
                    state: KeyState::Pressed,
                    ..
                } if !self.finished => {
                    let mode = if matches!(
                        event,
                        FamilyEvent::Key {
                            code: KeyCode::S,
                            ..
                        }
                    ) {
                        crate::MusicaPlayMode::Skip
                    } else {
                        crate::MusicaPlayMode::Auto
                    };
                    if self.vm.toggle_play_mode(mode).map_err(vm_error)? {
                        self.wait_ns = 0;
                    }
                    self.input_pending = false;
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
                FamilyEvent::WindowFocused { focused } => {
                    let changed = self.focused != *focused;
                    self.focused = *focused;
                    if !focused {
                        self.clear_input();
                    }
                    if changed && !self.progress_in_background {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
                }
                FamilyEvent::WindowVisibility { visible } => {
                    let changed = self.visible != *visible;
                    self.visible = *visible;
                    if !visible {
                        self.clear_input();
                    }
                    if changed {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
                }
                FamilyEvent::WindowSuspended { suspended } => {
                    let changed = self.suspended != *suspended;
                    if *suspended {
                        self.clear_input();
                    }
                    self.suspended = *suspended;
                    if changed {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
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
            self.quick_load()?;
        }
        let mut dirty = self.poll_text()? || choice_dirty;
        dirty |= self.advance_movie(elapsed_ns)?;
        if !self.paused()
            && self.movie.is_none()
            && !self.finished
            && self.vm.state().system_ui.page == crate::MusicaSystemPage::None
        {
            if self.vm.config().animation {
                dirty |= self
                    .vm
                    .advance_axis_scroll_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_linear_scroll_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_scroll_xf_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_wscroll2_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_firefly_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_character_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                if self.vm.config().screen_effect {
                    dirty |= self
                        .vm
                        .advance_effect_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                    dirty |= self
                        .vm
                        .advance_screen_shake_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                    dirty |= self
                        .vm
                        .advance_secondary_effect_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                }
            }
            dirty |= self
                .vm
                .advance_message_load_clock(elapsed_ns, self.vm.config().animation)
                .map_err(vm_error)?
                .is_some();
            self.phase += u128::from(elapsed_ns) * 60;
            while self.phase >= 1_000_000_000
                && !self.finished
                && self.vm.state().system_ui.page == crate::MusicaSystemPage::None
            {
                self.phase -= 1_000_000_000;
                dirty |= self.tick()?;
            }
        }
        if dirty && self.vm.state().system_ui.page == crate::MusicaSystemPage::Config {
            self.scene.render_config(
                self.vm.config_for_presentation().map_err(vm_error)?,
                self.pointer,
            )?;
        } else if dirty && self.vm.state().system_ui.page == crate::MusicaSystemPage::Title {
            self.scene
                .render_title(self.vm.title_variant(), self.title_focus)?;
        } else if dirty && crate::runtime::gallery::count(&self.vm.state().system_ui.page).is_some()
        {
            self.scene.render_gallery(
                &self.vm.state().system_ui.page,
                self.vm.state().system_ui.focus_index,
            )?;
        } else if dirty && self.is_save_page() {
            self.scene.render_save_page(
                self.vm.state().system_ui.page.clone(),
                self.vm.state().system_ui.focus_index,
                &self.save_cards,
            )?;
        } else if dirty && self.movie.is_none() {
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
            self.quick_save()?;
        }
        self.persist_progress()?;
        Ok(AdvanceResponse {
            reset_clock: std::mem::take(&mut self.reset_host_clock),
            status: if self.finished {
                FamilyStatus::Finished
            } else if self.vm.state().wait.is_some() {
                FamilyStatus::Waiting
            } else {
                FamilyStatus::Running
            },
            window_command: self
                .pending_fullscreen
                .take()
                .map(FamilyWindowCommand::SetFullscreen)
                .into(),
        })
    }
}
#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
