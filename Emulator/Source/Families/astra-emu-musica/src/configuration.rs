use crate::{MusicaPlayMode, MusicaRuntimeError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaConfigState {
    pub message_speed_unread: u8,
    pub message_speed_read: u8,
    /// Original `messageSpeedAutoPlay` setting in 10 ms units.
    pub message_speed_auto_play: u8,
    pub font_index: u32,
    pub preferred_play_mode: MusicaPlayMode,
    pub fullscreen: bool,
    pub screen_effect: bool,
    pub animation: bool,
    pub text_shadow: bool,
    pub backlog_voice_playback: bool,
    pub stop_voice_at_next_message: bool,
    pub progress_in_background: bool,
    pub bgm_volume: u8,
    pub voice_volume: u8,
    pub se_volume: u8,
    pub bgm_muted: bool,
    pub voice_muted: bool,
    pub se_muted: bool,
    /// Original order: ren, sui, aya, tou, etc.
    pub character_voice_enabled: [bool; 5],
}

impl MusicaConfigState {
    pub fn validate(&self) -> Result<(), MusicaRuntimeError> {
        validate_config_state(self)
    }
}

impl Default for MusicaConfigState {
    fn default() -> Self {
        Self {
            message_speed_unread: 50,
            message_speed_read: 50,
            message_speed_auto_play: 50,
            font_index: 0,
            preferred_play_mode: MusicaPlayMode::Auto,
            fullscreen: false,
            screen_effect: true,
            animation: true,
            text_shadow: true,
            backlog_voice_playback: true,
            stop_voice_at_next_message: false,
            progress_in_background: false,
            bgm_volume: 100,
            voice_volume: 100,
            se_volume: 100,
            bgm_muted: false,
            voice_muted: false,
            se_muted: false,
            character_voice_enabled: [true; 5],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicaConfigAudioBus {
    Bgm,
    Voice,
    Se,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicaConfigControl {
    MessageSpeedUnread(u8),
    MessageSpeedRead(u8),
    MessageSpeedAutoPlay(u8),
    FontPrevious,
    FontNext,
    PreferredPlayMode(MusicaPlayMode),
    Fullscreen(bool),
    ToggleScreenEffect,
    ToggleTextShadow,
    ToggleAnimation,
    ToggleBacklogVoicePlayback,
    ToggleStopVoiceAtNextMessage,
    ToggleProgressInBackground,
    BgmVolume(u8),
    VoiceVolume(u8),
    SeVolume(u8),
    ToggleBgmMute,
    ToggleVoiceMute,
    ToggleSeMute,
    TestAudio(MusicaConfigAudioBus),
    ToggleCharacterVoice(usize),
    Apply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicaConfigChange {
    Present,
    AudioParamsChanged,
    TestAudio(MusicaConfigAudioBus),
    Applied,
    Cancelled,
}

fn validate_config_state(config: &MusicaConfigState) -> Result<(), MusicaRuntimeError> {
    if config.message_speed_unread > 100
        || config.message_speed_read > 100
        || config.message_speed_auto_play > 100
        || config.font_index != 0
        || config.preferred_play_mode == MusicaPlayMode::Normal
        || config.bgm_volume > 100
        || config.voice_volume > 100
        || config.se_volume > 100
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}

impl MusicaConfigState {
    pub(crate) fn voice_preferences(&self) -> crate::voice_preferences::VoicePreferences {
        crate::voice_preferences::VoicePreferences {
            backlog_voice_playback: self.backlog_voice_playback,
            character_voice_enabled: self.character_voice_enabled,
        }
    }
    pub fn edit(
        &mut self,
        control: MusicaConfigControl,
    ) -> Result<MusicaConfigChange, MusicaRuntimeError> {
        let mut candidate = self.clone();
        let result = candidate.edit_inner(control)?;
        *self = candidate;
        Ok(result)
    }
    fn edit_inner(
        &mut self,
        control: MusicaConfigControl,
    ) -> Result<MusicaConfigChange, MusicaRuntimeError> {
        let draft = self;
        let change = match control {
            MusicaConfigControl::MessageSpeedUnread(value) => {
                draft.message_speed_unread = value;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::MessageSpeedRead(value) => {
                draft.message_speed_read = value;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::MessageSpeedAutoPlay(value) => {
                draft.message_speed_auto_play = value;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::FontPrevious => {
                draft.font_index = 0;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::FontNext => {
                draft.font_index = 0;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::PreferredPlayMode(mode) => {
                if mode == MusicaPlayMode::Normal {
                    return Err(MusicaRuntimeError::State);
                }
                draft.preferred_play_mode = mode;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::Fullscreen(value) => {
                draft.fullscreen = value;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleScreenEffect => {
                draft.screen_effect = !draft.screen_effect;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleTextShadow => {
                draft.text_shadow = !draft.text_shadow;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleAnimation => {
                draft.animation = !draft.animation;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleBacklogVoicePlayback => {
                draft.backlog_voice_playback = !draft.backlog_voice_playback;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleStopVoiceAtNextMessage => {
                draft.stop_voice_at_next_message = !draft.stop_voice_at_next_message;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::ToggleProgressInBackground => {
                draft.progress_in_background = !draft.progress_in_background;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::BgmVolume(value) => {
                draft.bgm_volume = value;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::VoiceVolume(value) => {
                draft.voice_volume = value;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::SeVolume(value) => {
                draft.se_volume = value;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::ToggleBgmMute => {
                draft.bgm_muted = !draft.bgm_muted;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::ToggleVoiceMute => {
                draft.voice_muted = !draft.voice_muted;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::ToggleSeMute => {
                draft.se_muted = !draft.se_muted;
                MusicaConfigChange::AudioParamsChanged
            }
            MusicaConfigControl::TestAudio(bus) => MusicaConfigChange::TestAudio(bus),
            MusicaConfigControl::ToggleCharacterVoice(index) => {
                let enabled = draft
                    .character_voice_enabled
                    .get_mut(index)
                    .ok_or(MusicaRuntimeError::State)?;
                *enabled = !*enabled;
                MusicaConfigChange::Present
            }
            MusicaConfigControl::Apply | MusicaConfigControl::Cancel => {
                return Err(MusicaRuntimeError::State)
            }
        };
        validate_config_state(draft)?;
        Ok(change)
    }
}
