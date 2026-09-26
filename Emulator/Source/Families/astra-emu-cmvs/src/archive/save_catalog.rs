use super::*;
use crate::{CmvsNativeSaveCatalog, CmvsNativeSaveSlot, CmvsPs2aVmState, CmvsStringSegment};

fn failure() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_SAVE_CATALOG",
        "native save header or path is invalid or unavailable",
    )
}

impl CmvsArchive {
    pub(crate) fn refresh_native_save_catalog(
        &self,
        vm: &mut CmvsPs2aVmState,
        count: u16,
    ) -> Result<(), CoreError> {
        if count > 256 {
            return Err(failure());
        }
        // Build the entire replacement first. A damaged or inaccessible slot
        // cannot partially replace the previous menu's authoritative state.
        let mut catalog = CmvsNativeSaveCatalog {
            slots: Vec::with_capacity(usize::from(count)),
            latest_slot: None,
        };
        let mut newest = 0;
        if count != 0 {
            if vm.interpreter_words.get(&10756).copied().unwrap_or(0) != 0 {
                return Err(failure());
            }
            let root = self.game_root.as_deref().ok_or_else(failure)?;
            let segments = vm
                .interpreter_string_buffers
                .get(&5404)
                .ok_or_else(failure)?;
            let [CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 }, CmvsStringSegment::OwnedString(prefix)] =
                segments.as_slice()
            else {
                return Err(failure());
            };
            native::validate_search_path(prefix)?;
            for slot in 0..count {
                let relative = format!("{}save{slot:03}.dat", prefix.replace('\\', "/"));
                let title = match std::fs::metadata(root.join(&relative)) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => return Err(failure()),
                    Ok(metadata) if metadata.is_file() => {
                        let path = astra_emu_sdk::resolve_game_file(root, &relative)?;
                        let bytes = native::read_native_range(&path, 0, metadata.len().min(512))?;
                        parse_header(&bytes)?
                    }
                    Ok(_) => return Err(failure()),
                };
                if let Some((_, timestamp)) = &title {
                    if *timestamp > newest {
                        newest = *timestamp;
                        catalog.latest_slot = Some(slot);
                    }
                }
                catalog.slots.push(CmvsNativeSaveSlot {
                    title: title.map(|(title, _)| title),
                });
            }
        }
        vm.native_save_catalog = Some(catalog);
        Ok(())
    }
}

fn parse_header(bytes: &[u8]) -> Result<Option<(String, u32)>, CoreError> {
    // Native menu eligibility accepts CSV2 through CSV9, independently of
    // the body codec. This is a catalog scan, never a claim the save can load.
    if bytes.len() < 4 || &bytes[..3] != b"CSV" || !(b'2'..=b'9').contains(&bytes[3]) {
        return Ok(None);
    }
    let bytes = bytes.get(16..).ok_or_else(failure)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(failure)?;
    let title = &bytes[..end];
    if title.len() < 19 {
        return Err(failure());
    }
    let text = encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(title)
        .ok_or_else(failure)?;
    Ok(Some((text.into_owned(), native_timestamp(title))))
}

// sub_4942A0 packs the original title's date characters directly. Preserve
// its u32 wrapping/masks and two-second precision; do not sort file mtimes
// or introduce a different calendar parser for the native menu.
fn native_timestamp(title: &[u8]) -> u32 {
    let a = |index| (title[index] as i8 as i32) as u32;
    let year = a(3).wrapping_add(10u32.wrapping_mul(a(2))).wrapping_sub(16);
    let day = a(9).wrapping_add(2u32.wrapping_mul(5u32.wrapping_mul(a(8)).wrapping_sub(8)));
    let hour = a(12)
        .wrapping_add(10u32.wrapping_mul(a(11)))
        .wrapping_sub(16);
    let minute = a(15).wrapping_add(10u32.wrapping_mul(a(14)));
    let second = a(18)
        .wrapping_add(10u32.wrapping_mul(a(17)))
        .wrapping_sub(16);
    ((a(6).wrapping_add(2042u32.wrapping_mul(a(5)))) << 21) & 0x01e0_0000
        | (32u32.wrapping_mul(minute).wrapping_sub(512)) & 0x7e0
        | ((hour & 0x1f | 32u32.wrapping_mul((year << 9) | (day & 0x1f))) << 11)
        | (second >> 1) & 0x1f
}

#[cfg(test)]
mod tests;
