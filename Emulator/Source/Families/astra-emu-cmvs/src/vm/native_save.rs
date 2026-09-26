use super::*;

/// Private native save-menu data. Titles never enter diagnostics or public reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSaveSlot {
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSaveCatalog {
    pub slots: Vec<CmvsNativeSaveSlot>,
    pub latest_slot: Option<u16>,
}

impl CmvsNativeSaveCatalog {
    pub fn load_enabled(&self, slot: u16) -> bool {
        self.slots
            .get(usize::from(slot))
            .is_some_and(|entry| entry.title.is_some())
    }

    pub fn save_enabled(&self, slot: u16) -> bool {
        usize::from(slot) < self.slots.len()
    }

    pub fn selected(&self, slot: u16) -> bool {
        self.latest_slot == Some(slot)
    }
}

impl CmvsPs2aVmState {
    /// Native sub_4728B0 requires both the live permission and saved flag.
    /// Menu/slot eligibility does not imply the native save codec is bound.
    pub fn native_save_enabled(&self) -> bool {
        self.interpreter_words.get(&1596).copied().unwrap_or(0) != 0
            && self.interpreter_words.get(&15064).copied().unwrap_or(0) != 0
    }
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    let Some(catalog) = &state.native_save_catalog else {
        return Ok(());
    };
    if catalog.slots.len() > 256
        || catalog
            .latest_slot
            .is_some_and(|slot| !catalog.load_enabled(slot))
        || catalog
            .slots
            .iter()
            .filter_map(|slot| slot.title.as_ref())
            .any(|title| {
                let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(title);
                invalid || encoded.len() < 19 || encoded.len() > 495 || encoded.contains(&0)
            })
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SAVE_CATALOG",
            "native save catalog is invalid",
        ));
    }
    Ok(())
}
