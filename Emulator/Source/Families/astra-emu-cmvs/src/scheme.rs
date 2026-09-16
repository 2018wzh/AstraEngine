use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CmvsSchemeProfile {
    pub version: u8,
    pub cpz5_secret: Vec<u32>,
    pub md5_variant: String,
    pub decoder_factor: u32,
    pub entry_init_key: u32,
    pub entry_sub_key: u32,
    pub entry_tail_key: u8,
    pub entry_key_pos: u8,
    pub index_seed: u32,
    pub index_addend: u32,
    pub index_subtrahend: u32,
    pub dir_key_addend: Vec<u32>,
}

impl CmvsSchemeProfile {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(5..=7).contains(&self.version)
            || self.cpz5_secret.len() != 24
            || !matches!(
                self.md5_variant.as_str(),
                "A" | "B" | "Chrono" | "Memoria" | "Natsu" | "Aoi" | "Mirai"
            )
            || self.entry_key_pos > 31
            || self.dir_key_addend.len() != 4
        {
            return Err("ASTRA_EMU_CMVS_PRIVATE_SCHEME");
        }
        Ok(())
    }
}
