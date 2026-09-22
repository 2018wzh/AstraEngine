//! The native message checkpoint subset whose fields have explicit semantics.
//! Opaque fields never survive this conversion: unsupported state is rejected.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct EdenCheckpoint {
    pub script: String,
    pub next_line: u32,
    pub message_id: i64,
    pub background: String,
    pub background_position: [i32; 2],
    pub panel_mode: u32,
    pub panel_resource: String,
    pub bgm: String,
    pub bgm_volume: u16,
    pub transition_ticks: u32,
    pub history: Vec<EdenHistoryMessage>,
}

/// Historical presentation is retained as named native state, not an opaque blob.
#[derive(Clone, PartialEq, Eq)]
pub struct EdenHistoryMessage {
    pub script: String,
    pub next_line: u32,
    pub message_id: i64,
    pub voice: String,
    pub speaker: String,
    pub text: String,
    pub background: String,
    pub background_position: [i32; 2],
    pub panel_mode: u32,
    pub panel_resource: String,
    pub bgm: String,
    pub bgm_volume: u16,
    pub sound_effects: [String; 2],
    pub transition_ticks: u32,
}

struct Fields<'a>(BTreeMap<&'a str, &'a str>);
impl<'a> Fields<'a> {
    fn new(fields: &'a [EdenSaveField]) -> Result<Self, CoreError> {
        let map: BTreeMap<_, _> = fields
            .iter()
            .map(|field| (field.name.as_str(), field.value.as_str()))
            .collect();
        if map.len() != fields.len() {
            return Err(unsupported());
        }
        Ok(Self(map))
    }
    fn take(&mut self, name: &str) -> Result<&'a str, CoreError> {
        self.0.remove(name).ok_or_else(unsupported)
    }
    fn number<T: std::str::FromStr>(&mut self, name: &str) -> Result<T, CoreError> {
        self.take(name)?.parse().map_err(|_| unsupported())
    }
    fn expect(&mut self, name: &str, value: &str) -> Result<(), CoreError> {
        if self.take(name)? != value {
            return Err(unsupported());
        }
        Ok(())
    }
    fn finish(self) -> Result<(), CoreError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(unsupported())
        }
    }
}

pub(super) fn unsupported() -> CoreError {
    invalid(
        "STATE",
        "native eden checkpoint contains unsupported or inconsistent state",
    )
}

impl EdenSave {
    /// Project the supported native message boundary. This does not execute scripts.
    pub fn checkpoint(&self) -> Result<EdenCheckpoint, CoreError> {
        if self.route != [0; 4] || self.backlog.is_empty() {
            return Err(unsupported());
        }
        let mut fields = Fields::new(&self.variables)?;
        for name in ["angBG", "angFG"] {
            fields.expect(name, "0000000000000000")?;
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
            fields.expect(name, "0")?;
        }
        for name in ["eff_Count", "eff_Param", "eff_Speed", "frameParam"] {
            fields.expect(name, "-1")?;
        }
        if let Some(method) = fields.0.remove("eff_Method") {
            if method != "end" {
                return Err(unsupported());
            }
        }
        fields.expect("font_Color", "16777215")?;
        fields.expect("font_FontSize", "22")?;
        fields.expect("font_RubySize", "10")?;
        let history = self
            .backlog
            .iter()
            .map(|record| EdenHistoryMessage::parse(record))
            .collect::<Result<Vec<_>, _>>()?;
        let last = history.last().ok_or_else(unsupported)?;
        let mut checkpoint = EdenCheckpoint {
            script: script_name(fields.take("script_Filename")?)?,
            next_line: fields.number("script_Pointer")?,
            message_id: fields.number("script_ID")?,
            background: resource_name(fields.take("stage_BGFilename")?)?,
            background_position: [fields.number("stage_BGX")?, fields.number("stage_BGY")?],
            panel_mode: fields.number("panel_Mode")?,
            // The native variable table omits the custom panel filename.
            panel_resource: last.panel_resource.clone(),
            bgm: resource_name(fields.take("bgm_Filename")?)?,
            bgm_volume: fields.number("bgm_Volume")?,
            transition_ticks: fields.number("tr_Param")?,
            history: Vec::new(),
        };
        fields.finish()?;
        if checkpoint.next_line == 0
            || checkpoint.bgm_volume > 100
            || checkpoint.script != last.script
            || checkpoint.next_line != last.next_line
            || checkpoint.message_id != last.message_id
            || checkpoint.background != last.background
            || checkpoint.background_position != last.background_position
            || checkpoint.panel_mode != last.panel_mode
            || checkpoint.bgm != last.bgm
            || checkpoint.bgm_volume != last.bgm_volume
            || checkpoint.transition_ticks != last.transition_ticks
            || last.sound_effects.iter().any(|name| !name.is_empty())
        {
            return Err(unsupported());
        }
        checkpoint.history = history;
        Ok(checkpoint)
    }
}

impl EdenHistoryMessage {
    fn parse(record: &[EdenSaveField]) -> Result<Self, CoreError> {
        let mut fields = Fields::new(record)?;
        fields.expect("LC", "0")?;
        fields.expect("CF1", "16777215")?;
        fields.expect("CF2", "22")?;
        fields.expect("CF3", "10")?;
        fields.expect("CT5", "10")?;
        for name in [
            "CT3", "CT6", "CT7", "bgH", "bgS", "bgV", "stH", "stS", "stV", "Ct1",
        ] {
            fields.expect(name, "0")?;
        }
        for name in [
            "CT1", "Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2", "Ce1", "Ce2", "Ct2",
        ] {
            fields.expect(name, "")?;
        }
        for name in ["FrP", "Ce3", "Ce4", "Ce5"] {
            fields.expect(name, "-1")?;
        }
        let message_id = fields.number("L0")?;
        if fields.number::<i64>("CS3")? != message_id {
            return Err(unsupported());
        }
        let message = Self {
            script: script_name(fields.take("CS1")?)?,
            next_line: fields.number("CS2")?,
            message_id,
            voice: fields.take("L1")?.into(),
            speaker: fields.take("L2")?.into(),
            text: fields.take("L3")?.into(),
            background: resource_name(fields.take("CT2")?)?,
            background_position: [0, fields.number("CT4")?],
            panel_mode: fields.number("CP1")?,
            panel_resource: resource_name(fields.take("CP2")?)?,
            bgm: resource_name(fields.take("CB1")?)?,
            bgm_volume: fields.number("CB1v")?,
            sound_effects: [
                resource_name(fields.take("CE1")?)?,
                resource_name(fields.take("CE2")?)?,
            ],
            transition_ticks: fields.number("Ct3")?,
        };
        fields.finish()?;
        if message.next_line == 0
            || message.bgm_volume > 100
            || !matches!(message.panel_mode, 0 | 1 | 3)
        {
            return Err(unsupported());
        }
        Ok(message)
    }
}

fn script_name(name: &str) -> Result<String, CoreError> {
    let value = resource_name(name)?;
    if !value.ends_with(".sc") {
        return Err(unsupported());
    }
    Ok(value)
}

fn resource_name(name: &str) -> Result<String, CoreError> {
    if name.len() > 256 || name.contains(['/', '\\', ':', '\0']) || matches!(name, "." | "..") {
        return Err(unsupported());
    }
    Ok(name.into())
}
