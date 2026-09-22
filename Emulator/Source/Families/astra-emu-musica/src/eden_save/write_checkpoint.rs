use super::*;

fn field(name: &str, value: impl ToString) -> EdenSaveField {
    EdenSaveField {
        name: name.into(),
        value: value.to_string(),
    }
}

impl EdenSave {
    pub(crate) fn from_history(
        edition: EdenEdition,
        encoding: EdenSaveEncoding,
        history: &[EdenHistoryMessage],
    ) -> Result<Self, CoreError> {
        if history.len() > 16_384 {
            return Err(checkpoint::unsupported());
        }
        let last = history.last().ok_or_else(checkpoint::unsupported)?;
        let mut variables = Vec::new();
        for name in ["angBG", "angFG"] {
            variables.push(field(name, "0000000000000000"));
        }
        for name in [
            "bgH",
            "bgS",
            "bgV",
            "stH",
            "stS",
            "stV",
            "stage_FGX",
            "stage_FGY",
            "rotBG_dest.x",
            "rotBG_dest.y",
            "rotBG_source.x",
            "rotBG_source.y",
            "rotFG_dest.x",
            "rotFG_dest.y",
            "rotFG_source.x",
            "rotFG_source.y",
            "tr_Method",
        ] {
            variables.push(field(name, 0));
        }
        for name in ["eff_Count", "eff_Param", "eff_Speed", "frameParam"] {
            variables.push(field(name, -1));
        }
        variables.extend([
            field("eff_Method", "end"),
            field("font_Color", 16777215),
            field("font_FontSize", 22),
            field("font_RubySize", 10),
            field("script_Filename", &last.script),
            field("script_Pointer", last.next_line),
            field("script_ID", last.message_id),
            field("stage_BGFilename", &last.background),
            field("stage_BGX", last.background_position[0]),
            field("stage_BGY", last.background_position[1]),
            field("panel_Mode", last.panel_mode),
            field("bgm_Filename", &last.bgm),
            field("bgm_Volume", last.bgm_volume),
            field("tr_Param", last.transition_ticks),
        ]);
        variables.sort_by(|left, right| left.name.cmp(&right.name));
        let save = Self {
            edition,
            encoding,
            comment: format!("Astra Musica {}", last.message_id),
            route: [0; 4],
            variables,
            backlog: history.iter().map(write_message).collect(),
        };
        // The writer must obey the same supported-state boundary as the reader.
        save.checkpoint()?;
        Ok(save)
    }
}

fn write_message(message: &EdenHistoryMessage) -> Vec<EdenSaveField> {
    vec![
        field("LC", 0),
        field("L0", message.message_id),
        field("L1", &message.voice),
        field("L2", &message.speaker),
        field("L3", &message.text),
        field("CS1", &message.script),
        field("CS2", message.next_line),
        field("CS3", message.message_id),
        field("CF1", 16777215),
        field("CF2", 22),
        field("CF3", 10),
        field("CP1", message.panel_mode),
        field("CP2", &message.panel_resource),
        field("CT1", ""),
        field("CT2", &message.background),
        field("CT3", message.background_position[0]),
        field("CT4", message.background_position[1]),
        field("CT5", 10),
        field("CT6", 0),
        field("CT7", 0),
        field("Frm", ""),
        field("FrP", -1),
        field("CO0", ""),
        field("CO1", ""),
        field("CO2", ""),
        field("Cs0", ""),
        field("Cs1", ""),
        field("Cs2", ""),
        field("bgH", 0),
        field("bgS", 0),
        field("bgV", 0),
        field("stH", 0),
        field("stS", 0),
        field("stV", 0),
        field("CB1", &message.bgm),
        field("CB1v", message.bgm_volume),
        field("CE1", &message.sound_effects[0]),
        field("CE2", &message.sound_effects[1]),
        field("Ce1", ""),
        field("Ce2", ""),
        field("Ce3", -1),
        field("Ce4", -1),
        field("Ce5", -1),
        field("Ct1", 0),
        field("Ct2", ""),
        field("Ct3", message.transition_ticks),
    ]
}
