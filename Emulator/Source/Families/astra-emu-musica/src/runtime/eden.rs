use super::*;
use crate::eden_save::{EdenHistoryMessage, EdenSave};
use crate::CoreError;
mod export;
pub(super) use export::{record_message, validate_context};

fn invalid() -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_EDEN_SAVE_STATE",
        "native eden state cannot be represented by this VM",
    )
}

impl MusicaVm {
    /// Load the saved script and all referenced history from the mounted game.
    pub fn from_eden_save(
        save: &EdenSave,
        archive: &crate::MusicaMountedVfs,
        session_seed: u64,
    ) -> Result<(Self, MusicaVmEvent), CoreError> {
        let checkpoint = save.checkpoint()?;
        let encoding = match save.encoding {
            crate::eden_save::EdenSaveEncoding::Gbk => crate::ScriptEncoding::Gbk,
            crate::eden_save::EdenSaveEncoding::ShiftJis => crate::ScriptEncoding::ShiftJis,
            crate::eden_save::EdenSaveEncoding::Windows1252 => return Err(invalid()),
        };
        let uri = format!("musica:/scr/{}", checkpoint.script);
        let loaded =
            crate::script_loader::load_script(archive, &uri, encoding).map_err(script_error)?;
        let mut vm =
            Self::new(uri, loaded.hash, loaded.script, session_seed).map_err(runtime_error)?;
        let event = vm.restore_eden_save(
            save,
            |name| {
                crate::script_loader::load_script(archive, &format!("musica:/scr/{name}"), encoding)
                    .map(|loaded| loaded.script)
                    .map_err(script_error)
            },
            1,
        )?;
        Ok((vm, event))
    }

    /// Prepare the entire restored state before replacing the active VM.
    /// The resolver only reads scripts; no command before the saved PC is run.
    pub fn restore_eden_save(
        &mut self,
        save: &EdenSave,
        mut read_script: impl FnMut(&str) -> Result<ScScript, CoreError>,
        next_fixed_tick: u64,
    ) -> Result<MusicaVmEvent, CoreError> {
        let checkpoint = save.checkpoint()?;
        if self.state.script_uri != format!("musica:/scr/{}", checkpoint.script)
            || next_fixed_tick == 0
        {
            return Err(invalid());
        }
        let position = save.validate_message_position(&checkpoint.script, &self.script)?;
        let mut scripts = BTreeMap::new();
        scripts.insert(checkpoint.script.clone(), self.script.clone());
        let mut script_bytes = script_size(&self.script)?;
        let mut candidate = Self::new(
            self.state.script_uri.clone(),
            self.state.script_hash,
            self.script.clone(),
            self.state.session_seed,
        )
        .map_err(runtime_error)?;
        candidate
            .set_config(self.config.clone())
            .map_err(runtime_error)?;
        candidate.state.launch_mode = self.state.launch_mode;
        candidate.state.transition = MusicaTransitionState {
            mode: 0,
            resource: None,
            duration_ticks: checkpoint.transition_ticks,
        };
        candidate.state.stage = Some(MusicaStageCommand {
            resource_sequence: vec![None],
            reference_position: None,
            background: (!checkpoint.background.is_empty()).then(|| MusicaStageLayer {
                resource_uri: format!("musica:/bg/{}", checkpoint.background),
                x: checkpoint.background_position[0],
                y: checkpoint.background_position[1],
            }),
            stands: Vec::new(),
            transition: candidate.state.transition.clone(),
        });
        candidate.state.panel = match checkpoint.panel_mode {
            0 => None,
            1 | 3 => Some(MusicaPanelState {
                mode: checkpoint.panel_mode,
                resource_uri: format!(
                    "musica:/sys/{}",
                    if checkpoint.panel_resource.is_empty() {
                        if checkpoint.panel_mode == 1 {
                            "msgPanel.png"
                        } else {
                            "fullPanel.png"
                        }
                    } else {
                        &checkpoint.panel_resource
                    }
                ),
            }),
            _ => return Err(invalid()),
        };
        if !checkpoint.bgm.is_empty() {
            let spec = parse_audio_resource_spec(&checkpoint.bgm).map_err(runtime_error)?;
            validate_audio_relative_path(&spec.resource).map_err(runtime_error)?;
            if spec.volume_percent != 100 || spec.pan_percent != 0 {
                return Err(invalid());
            }
            candidate.state.audio.insert(
                0,
                MusicaAudioState {
                    bus: "bgm".into(),
                    resource_uri: format!("musica:/bgm/{}", spec.resource),
                    looped: true,
                    volume_milli: checkpoint.bgm_volume * 10,
                    pan_milli: 0,
                    playing: true,
                    continuation_pts: 0,
                },
            );
        }
        let mut history_state = candidate.state.clone();
        for record in &checkpoint.history[..checkpoint.history.len() - 1] {
            if !scripts.contains_key(&record.script) {
                if scripts.len() >= 64 {
                    return Err(invalid());
                }
                let script = read_script(&record.script)?;
                script_bytes = script_bytes
                    .checked_add(script_size(&script)?)
                    .filter(|bytes| *bytes <= 64 * 1024 * 1024)
                    .ok_or_else(invalid)?;
                scripts.insert(record.script.clone(), script);
            }
            let script = scripts.get(&record.script).ok_or_else(invalid)?;
            let command = checked_command(record, script)?;
            // Decode only the recorded message, not its preceding execution path.
            history_state.message_loads.clear();
            history_state.audio.clear();
            history_state.backlog_bytes = 0;
            execute_message(
                command,
                &mut history_state,
                &self.config.voice_preferences(),
                self.config.message_speed_auto_play,
            )
            .map_err(runtime_error)?;
            let entry = history_state.backlog.pop().ok_or_else(invalid)?;
            backlog::append_backlog_entry(&mut candidate.state, entry).map_err(runtime_error)?;
        }
        let last = checkpoint.history.last().ok_or_else(invalid)?;
        let command = checked_command(last, &self.script)?;
        candidate.state.pc_line = position.next_line_index;
        candidate.state.instruction_count = 1;
        let event = execute_message(
            command,
            &mut candidate.state,
            &self.config.voice_preferences(),
            self.config.message_speed_auto_play,
        )
        .map_err(runtime_error)?
        .ok_or_else(invalid)?;
        // Native save has no clock for delayed character loads or auto-advance.
        if !candidate.state.message_loads.is_empty()
            || !matches!(candidate.state.wait, Some(MusicaWaitState::Input { .. }))
        {
            return Err(invalid());
        }
        candidate.state.fixed_tick = next_fixed_tick - 1;
        validate_stage_state(candidate.state.stage.as_ref()).map_err(runtime_error)?;
        backlog::validate_state(&candidate.state).map_err(runtime_error)?;
        candidate.state.eden_export = crate::eden_save::EdenExportState::Ready {
            edition: save.edition,
            encoding: save.encoding,
            history: checkpoint.history,
        };
        self.state = candidate.state;
        self.config_edit = None;
        Ok(event)
    }
}

fn script_size(script: &ScScript) -> Result<usize, CoreError> {
    script.lines.iter().try_fold(0usize, |size, line| {
        size.checked_add(line.raw.len())
            .filter(|bytes| *bytes <= 16 * 1024 * 1024)
            .ok_or_else(invalid)
    })
}

fn checked_command<'a>(
    record: &EdenHistoryMessage,
    script: &'a ScScript,
) -> Result<&'a ScCommand, CoreError> {
    let line = script
        .lines
        .get(record.next_line.checked_sub(1).ok_or_else(invalid)? as usize)
        .ok_or_else(invalid)?;
    let ScLineKind::Command { command } = &line.kind else {
        return Err(invalid());
    };
    if command.opcode != "message"
        || line
            .language_guard
            .is_some_and(|guard| guard != script.encoding.language())
    {
        return Err(invalid());
    }
    let tokens = command.tokens().map_err(|_| invalid())?;
    if tokens.len() < 4
        || tokens[0].parse::<i64>().ok() != Some(record.message_id)
        || tokens[1] != record.voice
        || tokens[2] != record.speaker
        || tokens[3..].join(" ") != record.text
    {
        return Err(invalid());
    }
    Ok(command)
}

fn script_error(cause: astra_emu_family_api::FamilyError) -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_EDEN_SAVE_SCRIPT",
        format!(
            "native checkpoint script could not be loaded: {}",
            cause.code()
        ),
    )
}

fn runtime_error(cause: MusicaRuntimeError) -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_EDEN_SAVE_STATE",
        format!(
            "native VM restoration rejected: {}",
            cause.diagnostic_code()
        ),
    )
}
