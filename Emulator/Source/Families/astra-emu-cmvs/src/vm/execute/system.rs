use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _start), (CmvsPs2aCommandStackWordKind::OpaqueU32, _count)],
        ) => {
            // No writable save files exist in the headless session, so the
            // newest-save scan returns the canonical "none" slot.
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SelectSystemCursor {
                manager_field_offset,
                current_index_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            let key = component_field_key(manager_field_offset, current_index_field_offset);
            if state.component_words.get(&key) == Some(index) {
                return Ok(None);
            }
            state.component_words.insert(key, *index);
            Ok(Some(CmvsPs2aVmAction::SelectSystemCursor { index: *index }))
        }
        (CmvsPs2aCommandEffectKind::RequestSystemStateLoad { path_buffer_offset }, []) => {
            let request = CmvsPs2aStorageRequest::LoadSystemState { path_buffer_offset };
            state.storage_await = Some(request.clone());
            Ok(Some(CmvsPs2aVmAction::StorageRequest(request)))
        }
        (
            CmvsPs2aCommandEffectKind::RecreateSaveImageOwner {
                handle_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            // `sub_47AFF0` frees the previous owner, then `sub_459BE0`
            // zeroes the flag and progress fields of the fresh owner. The
            // published handle word keeps the version-pinned field updated
            // for snapshot parity.
            state.save_image_owner = Some(CmvsSaveImageOwnerState {
                construction_words: [*first, *second, *third, *fourth],
                enabled: false,
                progress_delta: 0,
            });
            state.interpreter_words.insert(
                u32::from(handle_field_offset),
                state.save_image_owner.as_ref().map(|_| 1_u32).unwrap_or(0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageProgress {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_459CC0` returns the fourth construction word unchanged
            // when it is all-ones, otherwise reduced by the progress delta
            // the unrecovered render path updates.
            let value = match &state.save_image_owner {
                Some(owner) if owner.construction_words[3] == u32::MAX => u32::MAX,
                Some(owner) => owner.construction_words[3].wrapping_sub(owner.progress_delta),
                None => u32::MAX,
            };
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSaveImageEnabled,
            [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            // `sub_47B0F0` only touches the flag of a present owner; the
            // pop happens regardless of the owner's existence.
            if let Some(owner) = state.save_image_owner.as_mut() {
                owner.enabled = *enabled != 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, wait)],
        ) => match (&state.save_image_owner, *wait != 0) {
            // `sub_47B160` writes one for a missing owner and returns
            // 0x4004.
            (None, _) => {
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
            // The set-boolean branch waits for the screenshot render to
            // complete (dispatch result 0xA000); that path is not
            // recovered and blocks instead of spinning.
            (Some(_), true) => {
                state.interpreter_words.insert(result_field_offset, 0);
                Err(invalid(
                    "ASTRA_EMU_CMVS_VM_SAVE_IMAGE",
                    "CMVS screenshot completion wait is not recovered",
                ))
            }
            // The clear-boolean branch destroys the owner and reports
            // success (dispatch result 0xC004).
            (Some(_), false) => {
                state.save_image_owner = None;
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
        },
        (CmvsPs2aCommandEffectKind::DestroySaveImageOwner, []) => {
            // `sub_47B120` frees a present owner without further effects.
            state.save_image_owner = None;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageEnabled {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47B0B0` combines the owner existence with its field-5
            // flag through `sub_432F60`.
            let value = u32::from(
                state
                    .save_image_owner
                    .as_ref()
                    .is_some_and(|owner| owner.enabled),
            );
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::QueryRendererDisplayMode { .. }, []) => {
            // `sub_48A570` reads the inner display-mode fields of the
            // renderer object; that object model is not recovered and the
            // register values would branch the script, so block instead.
            Err(invalid(
                "ASTRA_EMU_CMVS_VM_DISPLAY_MODE",
                "CMVS renderer display-mode registers are not recovered",
            ))
        }
        (
            CmvsPs2aCommandEffectKind::QueryPointerPosition {
                x_field_offset,
                y_field_offset,
            },
            [],
        ) => {
            // `sub_480280` publishes the persistent pointer into system
            // registers 1 and 2 through `sub_45CF40`.
            state
                .interpreter_words
                .insert(x_field_offset, state.pointer_x as u32);
            state
                .interpreter_words
                .insert(y_field_offset, state.pointer_y as u32);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, x), (CmvsPs2aCommandStackWordKind::OpaqueU32, y), (CmvsPs2aCommandStackWordKind::OpaqueU32, width), (CmvsPs2aCommandStackWordKind::OpaqueU32, height)],
        ) => {
            // `sub_480060` rejects only when the pointer is strictly outside
            // the rectangle, so both bounds are inclusive.
            let x = *x as i32;
            let y = *y as i32;
            let hit = state.pointer_x >= x
                && state.pointer_x <= x.wrapping_add(*width as i32)
                && state.pointer_y >= y
                && state.pointer_y <= y.wrapping_add(*height as i32);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hit));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, shape), (CmvsPs2aCommandStackWordKind::OpaqueU32, left), (CmvsPs2aCommandStackWordKind::OpaqueU32, top), (CmvsPs2aCommandStackWordKind::OpaqueU32, right), (CmvsPs2aCommandStackWordKind::OpaqueU32, bottom), (CmvsPs2aCommandStackWordKind::OpaqueU32, _width), (CmvsPs2aCommandStackWordKind::OpaqueU32, _height)],
        ) => match *shape {
            // `SetRect` + `CreateRectRgnIndirect`: a GDI rectangle region is
            // half-open, covering left/top and excluding right/bottom.
            1 => {
                let hit = state.pointer_x >= *left as i32
                    && state.pointer_x < *right as i32
                    && state.pointer_y >= *top as i32
                    && state.pointer_y < *bottom as i32;
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(hit));
                Ok(None)
            }
            _ => Err(invalid(
                "ASTRA_EMU_CMVS_VM_POINTER_REGION",
                "CMVS pointer region shape rasterization is not recovered",
            )),
        },
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
