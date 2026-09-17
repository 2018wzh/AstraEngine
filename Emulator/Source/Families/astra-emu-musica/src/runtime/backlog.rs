use super::*;
use crate::validate_canonical_musica_message_text;
const MUSICA_BACKLOG_MAX_ENTRIES: usize = 16_384;
const MUSICA_BACKLOG_MAX_ENTRY_BYTES: usize = 64 * 1024;
const MUSICA_BACKLOG_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const VOICE_STREAM_ID: u32 = 4;
pub(super) fn append_backlog_entry(
    state: &mut MusicaRuntimeState,
    entry: MusicaBacklogEntry,
) -> Result<(), MusicaRuntimeError> {
    if state.backlog.len() >= MUSICA_BACKLOG_MAX_ENTRIES
        || entry.text.len() > MUSICA_BACKLOG_MAX_ENTRY_BYTES
        || entry
            .speaker
            .as_ref()
            .is_some_and(|speaker| speaker.len() > MUSICA_BACKLOG_MAX_ENTRY_BYTES)
    {
        return Err(MusicaRuntimeError::Backlog);
    }
    let entry_bytes = entry
        .text
        .len()
        .checked_add(entry.speaker.as_ref().map_or(0, String::len))
        .ok_or(MusicaRuntimeError::Backlog)?;
    let backlog_bytes = usize::try_from(state.backlog_bytes)
        .map_err(|_| MusicaRuntimeError::Backlog)?
        .checked_add(entry_bytes)
        .filter(|bytes| *bytes <= MUSICA_BACKLOG_MAX_TOTAL_BYTES)
        .ok_or(MusicaRuntimeError::Backlog)?;
    state.backlog.push(entry);
    state.backlog_bytes = u64::try_from(backlog_bytes).map_err(|_| MusicaRuntimeError::Backlog)?;
    Ok(())
}
pub(super) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.backlog.len() > MUSICA_BACKLOG_MAX_ENTRIES {
        return Err(MusicaRuntimeError::Backlog);
    }
    let mut total_bytes = 0usize;
    for entry in &state.backlog {
        validate_canonical_musica_message_text(&entry.text)
            .map_err(MusicaRuntimeError::MessageMarkup)?;
        if entry.text.len() > MUSICA_BACKLOG_MAX_ENTRY_BYTES
            || entry
                .speaker
                .as_ref()
                .is_some_and(|speaker| speaker.len() > MUSICA_BACKLOG_MAX_ENTRY_BYTES)
            || Hash256::from_sha256(entry.text.as_bytes()) != entry.text_hash
            || entry
                .speaker
                .as_ref()
                .map(|speaker| Hash256::from_sha256(speaker.as_bytes()))
                != entry.speaker_hash
            || entry.voice.is_some() != entry.voice_hash.is_some()
            || entry
                .voice
                .as_ref()
                .is_some_and(|voice| validate_message_voice(voice).is_err())
        {
            return Err(MusicaRuntimeError::Backlog);
        }
        total_bytes = total_bytes
            .checked_add(entry.text.len())
            .and_then(|bytes| bytes.checked_add(entry.speaker.as_ref().map_or(0, String::len)))
            .filter(|bytes| *bytes <= MUSICA_BACKLOG_MAX_TOTAL_BYTES)
            .ok_or(MusicaRuntimeError::Backlog)?;
    }
    if state.backlog_bytes != u64::try_from(total_bytes).map_err(|_| MusicaRuntimeError::Backlog)? {
        return Err(MusicaRuntimeError::Backlog);
    }
    match state.system_ui.page {
        MusicaSystemPage::Backlog => {
            let cursor = usize::try_from(
                state
                    .system_ui
                    .backlog_cursor
                    .ok_or(MusicaRuntimeError::Backlog)?,
            )
            .map_err(|_| MusicaRuntimeError::Backlog)?;
            if cursor >= state.backlog.len() {
                return Err(MusicaRuntimeError::Backlog);
            }
        }
        _ if state.system_ui.backlog_cursor.is_some() => {
            return Err(MusicaRuntimeError::Backlog);
        }
        _ => {}
    }
    Ok(())
}
fn validate_message_voice(voice: &MusicaMessageVoice) -> Result<(), MusicaRuntimeError> {
    let resource = voice
        .resource_uri
        .strip_prefix("musica:/voice/")
        .ok_or(MusicaRuntimeError::AudioResource)?;
    validate_audio_relative_path(resource)?;
    if voice.volume_milli > 1000 || !(-1000..=1000).contains(&voice.pan_milli) {
        return Err(MusicaRuntimeError::AudioResource);
    }
    Ok(())
}
impl MusicaVm {
    pub fn open_backlog(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::None
            || self.state.backlog.is_empty()
            || self.state.terminal
        {
            return Err(MusicaRuntimeError::Backlog);
        }
        let cursor =
            u32::try_from(self.state.backlog.len() - 1).map_err(|_| MusicaRuntimeError::Backlog)?;
        self.state.system_ui.page = MusicaSystemPage::Backlog;
        self.state.system_ui.focus_index = 0;
        self.state.system_ui.backlog_cursor = Some(cursor);
        Ok(())
    }
    pub fn move_backlog(&mut self, direction: i32) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Backlog || direction == 0 {
            return Err(MusicaRuntimeError::Backlog);
        }
        let last = u32::try_from(
            self.state
                .backlog
                .len()
                .checked_sub(1)
                .ok_or(MusicaRuntimeError::Backlog)?,
        )
        .map_err(|_| MusicaRuntimeError::Backlog)?;
        let cursor = self
            .state
            .system_ui
            .backlog_cursor
            .ok_or(MusicaRuntimeError::Backlog)?;
        self.state.system_ui.backlog_cursor = Some(if direction < 0 {
            cursor.saturating_sub(1)
        } else {
            cursor.saturating_add(1).min(last)
        });
        Ok(())
    }
    pub fn close_backlog(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Backlog {
            return Err(MusicaRuntimeError::Backlog);
        }
        self.state.system_ui.page = MusicaSystemPage::None;
        self.state.system_ui.focus_index = 0;
        self.state.system_ui.backlog_cursor = None;
        Ok(())
    }
    pub fn replay_backlog_voice(&mut self) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Backlog {
            return Err(MusicaRuntimeError::Backlog);
        }
        let cursor = usize::try_from(
            self.state
                .system_ui
                .backlog_cursor
                .ok_or(MusicaRuntimeError::Backlog)?,
        )
        .map_err(|_| MusicaRuntimeError::Backlog)?;
        let voice = self
            .state
            .backlog
            .get(cursor)
            .ok_or(MusicaRuntimeError::Backlog)?
            .voice
            .clone();
        let mut commands = Vec::new();
        if self
            .state
            .audio
            .get(&VOICE_STREAM_ID)
            .is_some_and(|current| current.playing)
        {
            commands.push(MusicaAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id: VOICE_STREAM_ID,
                fade_ms: 0,
            });
        }
        if let Some(voice) = voice {
            append_audio_load_and_play(
                &mut self.state,
                &mut commands,
                VOICE_STREAM_ID,
                &voice.resource_uri,
                voice.volume_milli,
                voice.pan_milli,
                false,
                0,
            )?;
            self.state.audio.insert(
                VOICE_STREAM_ID,
                MusicaAudioState {
                    bus: "voice".into(),
                    resource_uri: voice.resource_uri,
                    looped: false,
                    volume_milli: voice.volume_milli,
                    pan_milli: voice.pan_milli,
                    playing: true,
                    continuation_pts: 0,
                },
            );
        } else if let Some(current) = self.state.audio.get_mut(&VOICE_STREAM_ID) {
            current.playing = false;
        }
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backlog_rejects_corrupt_cursor_text_and_byte_accounting() {
        let source = b".message 1  speaker Hello\r\n.end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap();
        vm.step(1).unwrap();
        vm.open_backlog().unwrap();
        let saved = vm.encode_native_save().unwrap();
        for field in 0..3 {
            let mut state = vm.state().clone();
            match field {
                0 => state.system_ui.backlog_cursor = Some(1),
                1 => state.backlog_bytes += 1,
                _ => state.backlog[0].text.push('!'),
            }
            assert!(vm
                .restore_native_save(&postcard::to_allocvec(&state).unwrap(), 1)
                .is_err());
            assert_eq!(vm.encode_native_save().unwrap(), saved);
        }
        vm.move_backlog(-1).unwrap();
        vm.move_backlog(1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(0));
        vm.close_backlog().unwrap();
        assert!(vm.state().system_ui.backlog_cursor.is_none());
    }
}
