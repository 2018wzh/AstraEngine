use super::*;
use crate::eden_save::{EdenExportRejection as Rejection, EdenExportState, EdenHistoryMessage};

pub(in crate::runtime) fn validate_context(
    state: &MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    let EdenExportState::Ready {
        edition,
        encoding,
        history,
    } = &state.eden_export
    else {
        return Ok(());
    };
    if history.len() != state.backlog.len() {
        return Err(MusicaRuntimeError::NativeSaveFormat);
    }
    EdenSave::from_history(*edition, *encoding, history)
        .map_err(|_| MusicaRuntimeError::NativeSaveFormat)?;
    for (native, entry) in history.iter().zip(&state.backlog) {
        let text = crate::parse_musica_message_markup(&native.text)?.visible_text;
        let voice_hash =
            (!native.voice.is_empty()).then(|| Hash256::from_sha256(native.voice.as_bytes()));
        if native.message_id != entry.message_id
            || text != entry.text
            || (!native.speaker.is_empty()).then_some(&native.speaker) != entry.speaker.as_ref()
            || voice_hash != entry.voice_hash
        {
            return Err(MusicaRuntimeError::NativeSaveFormat);
        }
    }
    Ok(())
}

pub(in crate::runtime) fn record_message(state: &mut MusicaRuntimeState, command: &ScCommand) {
    if !matches!(state.eden_export, EdenExportState::Ready { .. }) {
        return;
    }
    match capture(state, command) {
        Ok(record) => {
            let EdenExportState::Ready { history, .. } = &mut state.eden_export else {
                return;
            };
            if history.len() >= 16_384 || history.len() + 1 != state.backlog.len() {
                state.eden_export = EdenExportState::Rejected(Rejection::HistoryBound);
            } else {
                history.push(record);
            }
        }
        Err(reason) => state.eden_export = EdenExportState::Rejected(reason),
    }
}

impl MusicaVm {
    /// Export only a currently representable message checkpoint. No native input
    /// fields are passed through, and this method never writes to a file.
    pub fn export_eden_save(&self) -> Result<EdenSave, CoreError> {
        validate_context(&self.state).map_err(runtime_error)?;
        let EdenExportState::Ready {
            edition,
            encoding,
            history,
        } = &self.state.eden_export
        else {
            return Err(invalid());
        };
        let last = history.last().ok_or_else(invalid)?;
        let command = checked_command(last, &self.script)?;
        let mut current = capture(&self.state, command).map_err(|_| invalid())?;
        // CT5 is the recorded panel transition setting, not a live timer.
        // The current message owns this typed value; new messages record zero.
        current.panel_fade = last.panel_fade;
        if history.len() != self.state.backlog.len() || current != *last {
            return Err(invalid());
        }
        let save = EdenSave::from_history(*edition, *encoding, history)?;
        save.encode()?;
        Ok(save)
    }
}

fn capture(
    state: &MusicaRuntimeState,
    command: &ScCommand,
) -> Result<EdenHistoryMessage, Rejection> {
    if !state.variables.is_empty() || !state.global_variables.is_empty() {
        return Err(Rejection::Variables);
    }
    if state.terminal
        || state.choice.is_some()
        || !matches!(state.wait, Some(MusicaWaitState::Input { .. }))
        || state
            .message
            .as_ref()
            .is_none_or(|message| message.source != command.span)
    {
        return Err(Rejection::MessageBoundary);
    }
    if state.effect.is_some()
        || state.movie.is_some()
        || state.firefly.is_some()
        || state.secondary_effect.is_some()
        || state.wscroll2.is_some()
        || state.scroll_xf.is_some()
        || state.linear_scroll.is_some()
        || state.axis_scroll.is_some()
        || state.screen_shake.is_some()
        || !state.characters.is_empty()
        || !state.message_loads.is_empty()
        || state.transition.mode != 0
        || state.transition.resource.is_some()
    {
        return Err(Rejection::Presentation);
    }
    let stage = state.stage.as_ref().ok_or(Rejection::Presentation)?;
    if stage.resource_sequence.as_slice() != [None]
        || stage
            .reference_position
            .is_some_and(|position| position != [0, 0])
        || !stage.stands.is_empty()
    {
        return Err(Rejection::Presentation);
    }
    let (background, background_position) = match &stage.background {
        Some(layer) if layer.x == 0 => (
            strip(&layer.resource_uri, "musica:/bg/")?,
            [layer.x, layer.y],
        ),
        None => (String::new(), [0, 0]),
        _ => return Err(Rejection::Presentation),
    };
    let (panel_mode, panel_resource) = match &state.panel {
        None => (0, String::new()),
        Some(panel) if panel.mode == 1 || panel.mode == 3 => {
            let name = strip(&panel.resource_uri, "musica:/sys/")?;
            let default = if panel.mode == 1 {
                "msgPanel.png"
            } else {
                "fullPanel.png"
            };
            (
                panel.mode,
                if name == default { String::new() } else { name },
            )
        }
        _ => return Err(Rejection::Presentation),
    };
    if state
        .audio
        .iter()
        .any(|(id, audio)| !matches!(*id, 0 | 4) && audio.playing)
    {
        return Err(Rejection::Audio);
    }
    let (bgm, bgm_volume) = match state.audio.get(&0).filter(|audio| audio.playing) {
        Some(audio)
            if audio.looped
                && audio.pan_milli == 0
                && audio.volume_milli <= 1000
                && audio.volume_milli.is_multiple_of(10) =>
        {
            (
                strip(&audio.resource_uri, "musica:/bgm/").map_err(|_| Rejection::Audio)?,
                audio.volume_milli / 10,
            )
        }
        None => (String::new(), 100),
        _ => return Err(Rejection::Audio),
    };
    let tokens = command.tokens().map_err(|_| Rejection::MessageBoundary)?;
    if tokens.len() < 4 {
        return Err(Rejection::MessageBoundary);
    }
    Ok(EdenHistoryMessage {
        script: strip(&state.script_uri, "musica:/scr/")?,
        next_line: state.pc_line,
        message_id: tokens[0].parse().map_err(|_| Rejection::MessageBoundary)?,
        voice: tokens[1].clone(),
        speaker: tokens[2].clone(),
        text: tokens[3..].join(" "),
        background,
        background_position,
        panel_mode,
        panel_resource,
        bgm,
        bgm_volume,
        sound_effects: [String::new(), String::new()],
        panel_fade: 0,
        transition_ticks: state.transition.duration_ticks,
    })
}

fn strip(uri: &str, prefix: &str) -> Result<String, Rejection> {
    let name = uri.strip_prefix(prefix).ok_or(Rejection::Presentation)?;
    if name.is_empty() || name.len() > 256 || name.contains(['/', '\\', ':', '\0']) {
        return Err(Rejection::Presentation);
    }
    Ok(name.into())
}
