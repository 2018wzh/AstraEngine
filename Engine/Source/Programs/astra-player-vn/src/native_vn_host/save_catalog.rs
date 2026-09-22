use super::*;

impl NativeVnHostCommandSource {
    /// Keep unreadable files occupied and protected without blocking other slots.
    pub fn reject_save_catalog_entry(&mut self, slot_id: &str) {
        self.ui_frame_reuse = None;
        if let Some(slot) = self.ui_save_slots.get_mut(slot_id) {
            slot.occupied = true;
            slot.can_write = false;
            slot.can_load = false;
            slot.has_thumbnail = false;
            slot.thumbnail_asset = None;
            slot.timestamp_text = None;
            slot.playtime_text = None;
            slot.metadata_text = Some("Unavailable save".into());
            slot.migration_status = "unavailable".into();
        }
    }

    pub(super) fn handle_quick_slot_input(
        &mut self,
        events: &[UiInputEvent],
    ) -> Result<bool, NativeVnHostError> {
        let Some((load, repeat)) = events.iter().find_map(|event| {
            let UiInputEventKind::Keyboard {
                physical_key,
                state: UiButtonState::Pressed,
                repeat,
                ..
            } = &event.kind
            else {
                return None;
            };
            match physical_key.as_str() {
                "F5" => Some((false, *repeat)),
                "F9" => Some((true, *repeat)),
                _ => None,
            }
        }) else {
            return Ok(false);
        };
        if repeat {
            return Ok(true);
        }
        let Some(slot_id) = self.system_ui_policy.quick_slot_id.clone() else {
            return Ok(true);
        };
        let Some(slot) = self.ui_save_slots.get(&slot_id) else {
            return Ok(true);
        };
        if (load && !slot.can_load) || (!load && !slot.can_write) {
            return Ok(true);
        }
        if self.pending_ui_host_request.is_some() {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_HOST_REQUEST_CONFLICT: a host request is already pending".into(),
            ));
        }
        self.pending_ui_host_request = Some(if load {
            VnUiHostRequest::Load { slot_id }
        } else {
            self.pending_save_completion = Some(SaveCompletionPolicy::Stay);
            VnUiHostRequest::Save {
                slot_id,
                completion: SaveCompletionPolicy::Stay,
            }
        });
        Ok(true)
    }
}
