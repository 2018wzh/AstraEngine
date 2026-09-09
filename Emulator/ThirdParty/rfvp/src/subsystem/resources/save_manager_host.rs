use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use anyhow::{anyhow, Result};

use crate::script::parser::Nls;
use crate::subsystem::resources::thread_manager::ThreadManagerSnapshotV1;
use crate::subsystem::save_state::{append_state_chunk_v1, SaveStateSnapshotV1};
use std::path::PathBuf;

const HOSTED_SAVE_MAGIC: [u8; 4] = *b"RFV9";
const HOSTED_SAVE_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDataFunction {
    RefreshAll,
    TestSaveData,
    DeleteSaveData,
    CopySaveData,
    GetSaveTitle,
    GetSaveSceneTitle,
    GetScriptContent,
    GetYear,
    GetMonth,
    GetDay,
    GetDayOfWeek,
    GetHour,
    GetMinute,
    LoadSaveThumbToTexture,
}

impl TryFrom<i32> for SaveDataFunction {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self> {
        match value {
            0 => Ok(Self::RefreshAll),
            1 => Ok(Self::TestSaveData),
            2 => Ok(Self::DeleteSaveData),
            3 => Ok(Self::CopySaveData),
            4 => Ok(Self::GetSaveTitle),
            5 => Ok(Self::GetSaveSceneTitle),
            6 => Ok(Self::GetScriptContent),
            7 => Ok(Self::GetYear),
            8 => Ok(Self::GetMonth),
            9 => Ok(Self::GetDay),
            10 => Ok(Self::GetDayOfWeek),
            11 => Ok(Self::GetHour),
            12 => Ok(Self::GetMinute),
            13 => Ok(Self::LoadSaveThumbToTexture),
            _ => Err(anyhow!("unknown SaveData function id: {value}")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SaveItem {
    pub title: String,
    pub scene_title: String,
    pub script_content: String,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub day_of_week: u8,
    pub hour: u8,
    pub minute: u8,
    pub thumb: Vec<u8>,
}

impl SaveItem {
    pub fn get_save_path(slot: u32) -> PathBuf {
        PathBuf::from(format!("save/save{slot:03}.dat").as_str())
    }

    pub fn resolve_save_path_for_read(slot: u32) -> PathBuf {
        Self::get_save_path(slot)
    }

    pub fn load_from_mem(buf: &[u8], _nls: Nls) -> Result<Self> {
        if buf.starts_with(&HOSTED_SAVE_MAGIC) {
            return decode_hosted_save(buf);
        }
        let text = core::str::from_utf8(buf).unwrap_or_default();
        let mut item = SaveItem::default();
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("title=") {
                item.title = value.to_string();
            } else if let Some(value) = line.strip_prefix("scene=") {
                item.scene_title = value.to_string();
            } else if let Some(value) = line.strip_prefix("script=") {
                item.script_content = value.to_string();
            }
        }
        Ok(item)
    }
}

#[derive(Debug)]
pub struct SaveManager {
    thumb_width: u32,
    thumb_height: u32,
    current_scene_title: String,
    current_title: String,
    current_script_content: String,
    current_save_slot: u32,
    savedata_requested: bool,
    savedata_prepared: bool,
    should_load: bool,
    save_requested: bool,
    local_saved: Option<Vec<u8>>,
    pending_vm_snapshot: Option<ThreadManagerSnapshotV1>,
    load_request: Option<u32>,
    slots: Vec<Option<SaveItem>>,
    slot_bytes: Vec<Option<Vec<u8>>>,
    file_operations: Vec<HostedSaveFileOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedSaveFileOperation {
    Refresh,
    Remove { slot: u32 },
    Copy { source: u32, destination: u32 },
}

impl Default for SaveManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SaveManager {
    pub fn new() -> Self {
        let mut slots = Vec::new();
        slots.resize_with(1000, || None);
        let mut slot_bytes = Vec::new();
        slot_bytes.resize_with(1000, || None);
        Self {
            thumb_width: 0,
            thumb_height: 0,
            current_scene_title: String::new(),
            current_title: String::new(),
            current_script_content: String::new(),
            current_save_slot: 0,
            savedata_requested: false,
            savedata_prepared: false,
            should_load: false,
            save_requested: false,
            local_saved: None,
            pending_vm_snapshot: None,
            load_request: None,
            slots,
            slot_bytes,
            file_operations: Vec::new(),
        }
    }

    pub fn set_thumb_size(&mut self, width: u32, height: u32) {
        self.thumb_width = width;
        self.thumb_height = height;
    }
    pub fn set_current_scene_title(&mut self, title: String) {
        self.current_scene_title = title;
    }
    pub fn set_current_title(&mut self, title: String) {
        self.current_title = title;
    }
    pub fn set_current_script_content(&mut self, content: String) {
        self.current_script_content = content;
    }
    pub fn set_current_save_slot(&mut self, slot: u32) {
        self.current_save_slot = slot;
    }
    pub fn set_savedata_requested(&mut self, requested: bool) {
        self.savedata_requested = requested;
    }
    pub fn set_savedata_prepared(&mut self, prepared: bool) {
        self.savedata_prepared = prepared;
    }
    pub fn set_should_load(&mut self, should_load: bool) {
        self.should_load = should_load;
    }

    pub fn get_thumb_width(&self) -> u32 {
        self.thumb_width
    }
    pub fn get_thumb_height(&self) -> u32 {
        self.thumb_height
    }
    pub fn get_current_scene_title(&self) -> &str {
        &self.current_scene_title
    }
    pub fn get_current_title(&self) -> &str {
        &self.current_title
    }
    pub fn get_current_script_content(&self) -> &str {
        &self.current_script_content
    }
    pub fn get_current_save_slot(&self) -> u32 {
        self.current_save_slot
    }
    pub fn is_save_requested(&self) -> bool {
        self.savedata_requested || self.save_requested
    }
    pub fn is_savedata_prepared(&self) -> bool {
        self.savedata_prepared
    }
    pub fn is_should_load(&self) -> bool {
        self.should_load
    }

    pub fn wants_vm_snapshot_capture(&self) -> bool {
        self.savedata_requested || self.local_saved.is_none() && self.savedata_prepared
    }

    pub fn set_pending_vm_snapshot(&mut self, snap: ThreadManagerSnapshotV1) {
        self.pending_vm_snapshot = Some(snap);
    }

    pub fn has_pending_vm_snapshot(&self) -> bool {
        self.pending_vm_snapshot.is_some()
    }

    pub fn take_pending_vm_snapshot(&mut self) -> Option<ThreadManagerSnapshotV1> {
        self.pending_vm_snapshot.take()
    }

    pub fn asynchronously_save(&mut self, slot: u32) {
        self.current_save_slot = slot;
        self.savedata_requested = true;
    }

    pub fn test_save_slot(&self, slot: u32) -> bool {
        self.slots.get(slot as usize).is_some_and(|s| s.is_some())
    }

    pub fn get_save_title(&self, slot: u32) -> String {
        self.slot(slot).map(|s| s.title.clone()).unwrap_or_default()
    }
    pub fn get_save_scene_title(&self, slot: u32) -> String {
        self.slot(slot)
            .map(|s| s.scene_title.clone())
            .unwrap_or_default()
    }
    pub fn get_script_content(&self, slot: u32) -> String {
        self.slot(slot)
            .map(|s| s.script_content.clone())
            .unwrap_or_default()
    }
    pub fn get_year(&self, slot: u32) -> u16 {
        self.slot(slot).map(|s| s.year).unwrap_or(0)
    }
    pub fn get_month(&self, slot: u32) -> u8 {
        self.slot(slot).map(|s| s.month).unwrap_or(0)
    }
    pub fn get_day(&self, slot: u32) -> u8 {
        self.slot(slot).map(|s| s.day).unwrap_or(0)
    }
    pub fn get_day_of_week(&self, slot: u32) -> u8 {
        self.slot(slot).map(|s| s.day_of_week).unwrap_or(0)
    }
    pub fn get_hour(&self, slot: u32) -> u8 {
        self.slot(slot).map(|s| s.hour).unwrap_or(0)
    }
    pub fn get_minute(&self, slot: u32) -> u8 {
        self.slot(slot).map(|s| s.minute).unwrap_or(0)
    }

    pub fn get_save_thumb(&self, slot: u32, width: u32, height: u32) -> Result<Vec<u8>> {
        if let Some(item) = self.slot(slot) {
            if !item.thumb.is_empty() {
                return Ok(item.thumb.clone());
            }
        }
        let len = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| anyhow!("save thumbnail size overflow"))? as usize;
        Ok(vec![0; len])
    }

    pub fn delete_savedata(&mut self, slot: u32) {
        if let Some(s) = self.slots.get_mut(slot as usize) {
            *s = None;
        }
        if let Some(s) = self.slot_bytes.get_mut(slot as usize) {
            *s = None;
        }
        self.file_operations
            .push(HostedSaveFileOperation::Remove { slot });
    }

    pub fn copy_savedata(&mut self, src: u32, dst: u32) -> Result<()> {
        let src_idx = src as usize;
        let dst_idx = dst as usize;
        if src_idx >= self.slots.len() || dst_idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        self.slots[dst_idx] = self.slots[src_idx].clone();
        self.slot_bytes[dst_idx] = self.slot_bytes[src_idx].clone();
        self.file_operations.push(HostedSaveFileOperation::Copy {
            source: src,
            destination: dst,
        });
        Ok(())
    }

    pub fn refresh_all_savedata(&mut self, _nls: Nls) -> Result<()> {
        self.file_operations.push(HostedSaveFileOperation::Refresh);
        Ok(())
    }

    pub fn take_file_operations(&mut self) -> Vec<HostedSaveFileOperation> {
        core::mem::take(&mut self.file_operations)
    }

    pub fn clear_cached_slots(&mut self) {
        self.slots.fill(None);
        self.slot_bytes.fill(None);
    }

    pub fn load_savedata(&mut self, slot: u32, nls: Nls) -> Result<()> {
        if let Some(Some(bytes)) = self.slot_bytes.get(slot as usize) {
            self.slots[slot as usize] = Some(SaveItem::load_from_mem(bytes, nls)?);
            return Ok(());
        }
        Err(anyhow!("save slot {slot} is empty"))
    }

    pub fn load_save_buff(&mut self, slot: u32, nls: Nls, cache: &Vec<u8>) -> Result<()> {
        self.load_slot_into_current_from_bytes(slot, nls, cache)
    }

    pub fn pending_save_capture(&self) -> Option<(u32, u32, u32)> {
        if self.savedata_requested {
            Some((self.current_save_slot, self.thumb_width, self.thumb_height))
        } else {
            None
        }
    }

    pub fn request_prepare_local_savedata(&mut self) {
        self.savedata_prepared = false;
        self.local_saved = None;
    }

    pub fn has_local_saved(&self) -> bool {
        self.local_saved.is_some()
    }

    pub fn finalize_local_savedata_prepare(&mut self, bytes: Vec<u8>, nls: Nls) -> Result<()> {
        let item = SaveItem::load_from_mem(&bytes, nls)?;
        self.local_saved = Some(bytes);
        self.savedata_prepared = true;
        let idx = self.current_save_slot as usize;
        if idx < self.slots.len() {
            self.slots[idx] = Some(item);
        }
        Ok(())
    }

    pub fn try_commit_local_savedata(&mut self, nls: Nls) -> Result<bool> {
        if !self.savedata_requested {
            return Ok(false);
        }
        let Some(bytes) = self.local_saved.clone() else {
            return Ok(false);
        };
        let idx = self.current_save_slot as usize;
        if idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        self.slots[idx] = Some(SaveItem::load_from_mem(&bytes, nls)?);
        self.slot_bytes[idx] = Some(bytes);
        self.savedata_requested = false;
        Ok(true)
    }

    pub fn finalize_save_write(&mut self, slot: u32, bytes: Vec<u8>, nls: Nls) -> Result<()> {
        let idx = slot as usize;
        if idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        self.slots[idx] = Some(SaveItem::load_from_mem(&bytes, nls)?);
        self.slot_bytes[idx] = Some(bytes);
        self.savedata_requested = false;
        Ok(())
    }

    pub fn consume_save_write_result(&mut self) {
        self.savedata_requested = false;
    }

    pub fn request_load(&mut self, slot: u32) {
        self.load_request = Some(slot);
        self.should_load = true;
    }

    pub fn take_load_request(&mut self) -> Option<u32> {
        let out = self.load_request.take();
        if out.is_some() {
            self.should_load = false;
        }
        out
    }

    pub fn load_slot_into_current(&mut self, slot: u32, nls: Nls) -> Result<()> {
        self.load_savedata(slot, nls)
    }

    pub fn load_slot_into_current_from_bytes(
        &mut self,
        slot: u32,
        nls: Nls,
        bytes: &[u8],
    ) -> Result<()> {
        let idx = slot as usize;
        if idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        self.slots[idx] = Some(SaveItem::load_from_mem(bytes, nls)?);
        self.slot_bytes[idx] = Some(bytes.to_vec());
        Ok(())
    }

    pub fn finish_save_write_from_thumb(
        &mut self,
        slot: u32,
        mut bytes: Vec<u8>,
        thumb: Vec<u8>,
        nls: Nls,
    ) -> Result<()> {
        let item = SaveItem {
            title: self.current_title.clone(),
            scene_title: self.current_scene_title.clone(),
            script_content: self.current_script_content.clone(),
            thumb,
            ..SaveItem::default()
        };
        bytes.extend_from_slice(
            format!(
                "\ntitle={}\nscene={}\nscript={}\n",
                item.title, item.scene_title, item.script_content
            )
            .as_bytes(),
        );
        let idx = slot as usize;
        if idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        self.slots[idx] = Some(SaveItem::load_from_mem(&bytes, nls).unwrap_or(item));
        self.slot_bytes[idx] = Some(bytes);
        self.savedata_requested = false;
        Ok(())
    }

    pub fn build_and_store_hosted_save(
        &mut self,
        slot: u32,
        thumb: Vec<u8>,
        snapshot: &SaveStateSnapshotV1,
        nls: Nls,
    ) -> Result<Vec<u8>> {
        let idx = slot as usize;
        if idx >= self.slots.len() {
            return Err(anyhow!("save slot out of range"));
        }
        let item = SaveItem {
            title: self.current_title.clone(),
            scene_title: self.current_scene_title.clone(),
            script_content: self.current_script_content.clone(),
            thumb,
            ..SaveItem::default()
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&HOSTED_SAVE_MAGIC);
        bytes.extend_from_slice(&HOSTED_SAVE_VERSION.to_le_bytes());
        push_field(&mut bytes, item.title.as_bytes())?;
        push_field(&mut bytes, item.scene_title.as_bytes())?;
        push_field(&mut bytes, item.script_content.as_bytes())?;
        push_field(&mut bytes, &item.thumb)?;
        append_state_chunk_v1(&mut bytes, snapshot)?;
        self.slots[idx] = Some(SaveItem::load_from_mem(&bytes, nls).unwrap_or(item));
        self.slot_bytes[idx] = Some(bytes.clone());
        self.savedata_requested = false;
        self.savedata_prepared = true;
        self.local_saved = Some(bytes.clone());
        Ok(bytes)
    }

    fn slot(&self, slot: u32) -> Option<&SaveItem> {
        self.slots.get(slot as usize).and_then(|s| s.as_ref())
    }
}

fn push_field(output: &mut Vec<u8>, field: &[u8]) -> Result<()> {
    let length = u32::try_from(field.len()).map_err(|_| anyhow!("save field too large"))?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(field);
    Ok(())
}

fn decode_hosted_save(bytes: &[u8]) -> Result<SaveItem> {
    let mut cursor = 4usize;
    let version = read_u16(bytes, &mut cursor)?;
    if version != HOSTED_SAVE_VERSION {
        return Err(anyhow!("unsupported hosted save version: {version}"));
    }
    let title = read_field(bytes, &mut cursor)?;
    let scene_title = read_field(bytes, &mut cursor)?;
    let script_content = read_field(bytes, &mut cursor)?;
    let thumb = read_field(bytes, &mut cursor)?.to_vec();
    Ok(SaveItem {
        title: core::str::from_utf8(title)?.to_string(),
        scene_title: core::str::from_utf8(scene_title)?.to_string(),
        script_content: core::str::from_utf8(script_content)?.to_string(),
        thumb,
        ..SaveItem::default()
    })
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16> {
    let end = cursor
        .checked_add(2)
        .ok_or_else(|| anyhow!("save cursor overflow"))?;
    let field = bytes
        .get(*cursor..end)
        .ok_or_else(|| anyhow!("save header truncated"))?;
    *cursor = end;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn read_field<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let length_end = cursor
        .checked_add(4)
        .ok_or_else(|| anyhow!("save cursor overflow"))?;
    let length = bytes
        .get(*cursor..length_end)
        .ok_or_else(|| anyhow!("save field length truncated"))?;
    *cursor = length_end;
    let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| anyhow!("save field length overflow"))?;
    let field = bytes
        .get(*cursor..end)
        .ok_or_else(|| anyhow!("save field truncated"))?;
    *cursor = end;
    Ok(field)
}
