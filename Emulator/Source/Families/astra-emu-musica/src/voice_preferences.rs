use astra_emu_family_api::*;

const FIELDS: [(&str, &str); 6] = [
    ("backlog_voice_playback", "Backlog voice playback"),
    ("voice_ren", "Ren voice"),
    ("voice_sui", "Sui voice"),
    ("voice_aya", "Aya voice"),
    ("voice_tou", "Tou voice"),
    ("voice_mot", "Other character voice"),
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
    pub fn fields() -> Vec<ConfigField> {
        FIELDS
            .iter()
            .map(|(id, label)| ConfigField {
                id: (*id).into(),
                label: (*label).into(),
                group: "Voice".into(),
                kind: ConfigKind::Bool,
                default: ConfigValue::Bool(true),
            })
            .collect()
    }
    pub fn resolve(entries: &[ConfigEntry]) -> FamilyResult<Self> {
        let mut flags = [true; 6];
        for (index, (id, _)) in FIELDS.iter().enumerate() {
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
