use astra_emu_family_api::*;

const BUSES: [(&str, &str); 3] = [("bgm", "BGM"), ("voice", "Voice"), ("se", "Sound effects")];

/// User bus gain is separate from authored volume and never stored in a save slot.
#[derive(Clone, Debug)]
pub(crate) struct AudioPreferences {
    pub volume: [u8; 3],
    pub muted: [bool; 3],
}
impl Default for AudioPreferences {
    fn default() -> Self {
        Self {
            volume: [100; 3],
            muted: [false; 3],
        }
    }
}
impl AudioPreferences {
    pub fn fields() -> Vec<ConfigField> {
        BUSES
            .iter()
            .flat_map(|(id, label)| {
                [
                    ConfigField {
                        id: format!("{id}_volume").into(),
                        label: format!("{label} volume").into(),
                        group: "Audio".into(),
                        kind: ConfigKind::Integer { min: 0, max: 100 },
                        default: ConfigValue::Integer(100),
                    },
                    ConfigField {
                        id: format!("{id}_muted").into(),
                        label: format!("Mute {label}").into(),
                        group: "Audio".into(),
                        kind: ConfigKind::Bool,
                        default: ConfigValue::Bool(false),
                    },
                ]
            })
            .collect()
    }
    pub fn resolve(entries: &[ConfigEntry]) -> FamilyResult<Self> {
        let mut result = Self::default();
        for (index, (id, _)) in BUSES.iter().enumerate() {
            let volume = format!("{id}_volume");
            let mute = format!("{id}_muted");
            result.volume[index] = match entries.iter().find(|e| e.id == volume).map(|e| &e.value) {
                Some(ConfigValue::Integer(value)) if (0..=100).contains(value) => *value as u8,
                _ => return Err(invalid()),
            };
            result.muted[index] = match entries.iter().find(|e| e.id == mute).map(|e| &e.value) {
                Some(ConfigValue::Bool(value)) => *value,
                _ => return Err(invalid()),
            };
        }
        Ok(result)
    }
    pub fn gains(&self) -> FamilyResult<[f32; 3]> {
        if self.volume.iter().any(|v| *v > 100) {
            return Err(invalid());
        }
        Ok(std::array::from_fn(|i| {
            if self.muted[i] {
                0.0
            } else {
                f32::from(self.volume[i]) / 100.0
            }
        }))
    }
}
fn invalid() -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_MUSICA_CONFIG",
        "audio preference is missing or invalid",
    )
}

pub(super) fn bus_index(uri: &str) -> FamilyResult<usize> {
    match uri {
        "musica:/sys/BGMTest.wav" => return Ok(0),
        "musica:/sys/VOICEtest.wav" => return Ok(1),
        "musica:/sys/SEtest.wav" => return Ok(2),
        _ => {}
    }
    match uri
        .strip_prefix("musica:/")
        .and_then(|s| s.split_once('/'))
        .map(|(role, _)| role)
    {
        Some("bgm") => Ok(0),
        Some("voice") => Ok(1),
        Some("se") => Ok(2),
        _ => Err(FamilyError::invalid(
            "ASTRA_EMU_MUSICA_AUDIO_BUS",
            "audio resource has an unsupported bus",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_audio_fields_resolve_defaults_and_reject_invalid_values() {
        let descriptor = crate::musica_descriptor();
        descriptor.validate().unwrap();
        let defaults = resolve_config(&descriptor.configuration, &[]).unwrap();
        assert_eq!(
            AudioPreferences::resolve(&defaults)
                .unwrap()
                .gains()
                .unwrap(),
            [1.0; 3]
        );
        let entries = resolve_config(
            &descriptor.configuration,
            &[
                ConfigEntry {
                    id: "bgm_volume".into(),
                    value: ConfigValue::Integer(40),
                },
                ConfigEntry {
                    id: "voice_muted".into(),
                    value: ConfigValue::Bool(true),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            AudioPreferences::resolve(&entries)
                .unwrap()
                .gains()
                .unwrap(),
            [0.4, 0.0, 1.0]
        );
        for value in [-1, 101] {
            assert!(resolve_config(
                &descriptor.configuration,
                &[ConfigEntry {
                    id: "se_volume".into(),
                    value: ConfigValue::Integer(value)
                }]
            )
            .is_err());
        }
        assert!(AudioPreferences::resolve(&[]).is_err());
        for (uri, index) in [
            ("musica:/bgm/tone.ogg", 0),
            ("musica:/voice/a.ogg", 1),
            ("musica:/se/b.ogg", 2),
        ] {
            assert_eq!(bus_index(uri).unwrap(), index);
        }
        assert!(bus_index("musica:/unknown/a.ogg").is_err());
    }
}
