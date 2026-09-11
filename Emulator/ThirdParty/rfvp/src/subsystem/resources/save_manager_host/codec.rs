use super::SaveItem;
use crate::host_api::clock::CalendarTime;
use crate::script::parser::Nls;
use crate::subsystem::save_state::{append_state_chunk_v1, SaveStateSnapshotV1};
use alloc::{string::String, vec::Vec};
use anyhow::{anyhow, bail, Result};

fn encoding(nls: Nls) -> &'static encoding_rs::Encoding {
    match nls {
        Nls::ShiftJIS => encoding_rs::SHIFT_JIS,
        Nls::GBK => encoding_rs::GBK,
        Nls::UTF8 => encoding_rs::UTF_8,
    }
}

pub(super) fn encode(item: &SaveItem, nls: Nls, snapshot: &SaveStateSnapshotV1) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&item.year.to_le_bytes());
    bytes.extend_from_slice(&[
        item.month,
        item.day,
        item.day_of_week,
        item.hour,
        item.minute,
    ]);
    let encoding = encoding(nls);
    for text in [&item.title, &item.scene_title, &item.script_content] {
        let (field, _, errors) = encoding.encode(text);
        if errors {
            bail!("save text cannot be encoded");
        }
        let length = u16::try_from(field.len()).map_err(|_| anyhow!("save text is too long"))?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&field);
    }
    bytes.extend_from_slice(&item.thumb);
    append_state_chunk_v1(&mut bytes, snapshot)?;
    Ok(bytes)
}

// Slot enumeration reads native metadata only. VM compatibility is checked when
// a slot is loaded, without deserializing every scene while opening the menu.
pub(super) fn decode(bytes: &[u8], nls: Nls) -> Result<SaveItem> {
    if !bytes.ends_with(b"RFVS") {
        bail!("save VM state is missing");
    }
    let footer = bytes
        .len()
        .checked_sub(8)
        .ok_or_else(|| anyhow!("save footer truncated"))?;
    let state_len = u32::from_le_bytes(bytes[footer..footer + 4].try_into()?) as usize;
    let prefix_end = footer
        .checked_sub(state_len)
        .ok_or_else(|| anyhow!("save state length invalid"))?;
    let mut prefix = &bytes[..prefix_end];
    let date = take(&mut prefix, 7)?;
    let calendar = CalendarTime {
        year: u16::from_le_bytes([date[0], date[1]]),
        month: date[2],
        day: date[3],
        day_of_week: date[4],
        hour: date[5],
        minute: date[6],
    };
    calendar
        .validate()
        .map_err(|_| anyhow!("save date invalid"))?;
    let encoding = encoding(nls);
    let mut text = || -> Result<String> {
        let length = u16::from_le_bytes(take(&mut prefix, 2)?.try_into()?) as usize;
        let (text, errors) = encoding.decode_without_bom_handling(take(&mut prefix, length)?);
        if errors {
            bail!("save text encoding invalid");
        }
        Ok(text.into_owned())
    };
    let title = text()?;
    let scene_title = text()?;
    let script_content = text()?;
    if prefix.len() % 4 != 0 {
        bail!("save thumbnail is not RGBA8");
    }
    Ok(SaveItem {
        title,
        scene_title,
        script_content,
        year: calendar.year,
        month: calendar.month,
        day: calendar.day,
        day_of_week: calendar.day_of_week,
        hour: calendar.hour,
        minute: calendar.minute,
        thumb: prefix.to_vec(),
    })
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8]> {
    let field = bytes
        .get(..length)
        .ok_or_else(|| anyhow!("save header truncated"))?;
    *bytes = &bytes[length..];
    Ok(field)
}
