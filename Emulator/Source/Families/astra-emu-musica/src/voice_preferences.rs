use astra_emu_family_api::*;

const FIELDS: [(&str, &str, &str); 6] = [
    (
        "backlog_voice_playback",
        "回想中播放语音",
        "是否允许在回想记录中重播角色语音；游戏内尚未保存设置时作为初始值。",
    ),
    (
        "voice_ren",
        "Ren 的语音",
        "是否播放语音文件名以 ren 开头的角色语音；游戏内尚未保存设置时作为初始值。",
    ),
    (
        "voice_sui",
        "Sui 的语音",
        "是否播放语音文件名以 sui 开头的角色语音；游戏内尚未保存设置时作为初始值。",
    ),
    (
        "voice_aya",
        "Aya 的语音",
        "是否播放语音文件名以 aya 开头的角色语音；游戏内尚未保存设置时作为初始值。",
    ),
    (
        "voice_tou",
        "Tou 的语音",
        "是否播放语音文件名以 tou 开头的角色语音；游戏内尚未保存设置时作为初始值。",
    ),
    (
        "voice_mot",
        "其他角色语音",
        "是否播放语音文件名以 mot 开头的其他角色语音；游戏内尚未保存设置时作为初始值。",
    ),
];

// Startup preferences belong to Manager configuration, not a game save slot.
#[derive(Clone, Debug)]
pub(crate) struct VoicePreferences {
    pub backlog_voice_playback: bool,
    pub character_voice_enabled: [bool; 5],
}
impl Default for VoicePreferences {
    fn default() -> Self {
        Self {
            backlog_voice_playback: true,
            character_voice_enabled: [true; 5],
        }
    }
}
impl VoicePreferences {
    pub(crate) fn fields() -> Vec<crate::provider::MusicaField> {
        FIELDS
            .iter()
            .map(|(id, label, description)| {
                crate::provider::MusicaField::new(
                    id,
                    label,
                    "初始语音设置",
                    ConfigKind::Bool,
                    ConfigValue::Bool(true),
                    description,
                )
            })
            .collect()
    }
    pub fn resolve(entries: &[ConfigEntry]) -> FamilyResult<Self> {
        let mut flags = [true; 6];
        for (index, (id, _, _)) in FIELDS.iter().enumerate() {
            flags[index] = match entries
                .iter()
                .find(|entry| entry.id == *id)
                .map(|entry| &entry.value)
            {
                Some(ConfigValue::Bool(value)) => *value,
                _ => {
                    return Err(FamilyError::invalid(
                        "ASTRA_EMU_MUSICA_CONFIG",
                        "voice preference is missing or invalid",
                    ))
                }
            };
        }
        Ok(Self {
            backlog_voice_playback: flags[0],
            character_voice_enabled: flags[1..].try_into().unwrap(),
        })
    }
    pub fn enabled(&self, uri: &str) -> bool {
        let Some(name) = uri.strip_prefix("musica:/voice/") else {
            return true;
        };
        let index = match name.split('-').next().unwrap_or_default() {
            "ren" => 0,
            "sui" => 1,
            "aya" => 2,
            "tou" => 3,
            "mot" => 4,
            _ => return true,
        };
        self.character_voice_enabled[index]
    }
}
