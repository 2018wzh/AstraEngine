use super::*;
use crate::{runtime::gallery, MusicaSystemPage};
impl MusicaSession {
    fn activate_gallery(&mut self) -> FamilyResult<()> {
        let focus = self.vm.state().system_ui.focus_index;
        match self.vm.state().system_ui.page {
            MusicaSystemPage::Memories => {
                let page = match focus {
                    0 => MusicaSystemPage::GalleryCg,
                    1 => MusicaSystemPage::GalleryReplay,
                    2 => MusicaSystemPage::GalleryBgm,
                    3 => MusicaSystemPage::GalleryMovie,
                    4 => MusicaSystemPage::Title,
                    _ => {
                        return Err(error(
                            "ASTRA_EMU_MUSICA_GALLERY_FOCUS",
                            "invalid Memories focus",
                        ))
                    }
                };
                self.vm.set_gallery_page(page, 0).map_err(vm_error)
            }
            MusicaSystemPage::GalleryBgm => {
                let commands = self.vm.gallery_bgm_play().map_err(vm_error)?;
                self.audio.apply(commands)
            }
            MusicaSystemPage::GalleryReplay | MusicaSystemPage::GalleryMovie => {
                let scripts = if self.vm.state().system_ui.page == MusicaSystemPage::GalleryReplay {
                    gallery::REPLAYS
                } else {
                    gallery::MOVIES
                };
                let target = scripts.get(focus as usize).ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_GALLERY_FOCUS",
                        "invalid script gallery focus",
                    )
                })?;
                self.start_menu_script(format!("musica:/scr/{target}"))
            }
            _ => Ok(()),
        }
    }
    pub(super) fn gallery_event(&mut self, event: &FamilyEvent) -> FamilyResult<bool> {
        let page = self.vm.state().system_ui.page.clone();
        if gallery::count(&page).is_none() {
            return Ok(false);
        }
        match event {
            FamilyEvent::Key {
                code: KeyCode::Escape,
                state: KeyState::Pressed,
                ..
            } => {
                if page == MusicaSystemPage::GalleryBgm {
                    let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
                    self.audio.apply(commands)?;
                }
                self.vm
                    .set_gallery_page(
                        if page == MusicaSystemPage::Memories {
                            MusicaSystemPage::Title
                        } else {
                            MusicaSystemPage::Memories
                        },
                        0,
                    )
                    .map_err(vm_error)?;
                self.title_focus = None;
            }
            FamilyEvent::Key {
                code: KeyCode::ArrowUp,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_gallery_focus(-1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_gallery_focus(1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::ArrowLeft,
                state: KeyState::Pressed,
                ..
            } if page == MusicaSystemPage::GalleryBgm => {
                self.vm.move_gallery_focus(-16).map_err(vm_error)?
            }
            FamilyEvent::Key {
                code: KeyCode::ArrowRight,
                state: KeyState::Pressed,
                ..
            } if page == MusicaSystemPage::GalleryBgm => {
                self.vm.move_gallery_focus(16).map_err(vm_error)?
            }
            FamilyEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => self.activate_gallery()?,
            FamilyEvent::PointerMove { x, y } => self.pointer = Some((*x, *y)),
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } if page == MusicaSystemPage::GalleryBgm => {
                if let Some((x, y)) = self.pointer {
                    self.gallery_bgm_pointer(x, y)?;
                }
            }
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => return Ok(false),
            _ => {}
        }
        tracing::debug!(event="astra.emu.musica.gallery.input",page=?self.vm.state().system_ui.page,focus=self.vm.state().system_ui.focus_index);
        Ok(true)
    }
    fn gallery_bgm_pointer(&mut self, x: f32, y: f32) -> FamilyResult<()> {
        if (140.0..420.0).contains(&x) && (96.0..592.0).contains(&y) {
            let focus =
                self.vm.state().system_ui.focus_index / 16 * 16 + ((y - 96.0) / 32.0) as u32;
            if focus >= 47 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_GALLERY_BGM_POINTER",
                    "BGM gallery pointer selected an empty row",
                ));
            }
            self.vm
                .set_gallery_page(MusicaSystemPage::GalleryBgm, focus)
                .map_err(vm_error)?;
            return self.activate_gallery();
        }
        if (558.0..628.0).contains(&y) {
            if (578.0..644.0).contains(&x) {
                self.vm.move_gallery_focus(-1).map_err(vm_error)?;
            } else if (656.0..722.0).contains(&x) {
                self.activate_gallery()?;
            } else if (736.0..802.0).contains(&x) {
                let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
                self.audio.apply(commands)?;
            } else if (816.0..882.0).contains(&x) {
                self.vm.move_gallery_focus(1).map_err(vm_error)?;
            }
        }
        if (650.0..710.0).contains(&y) {
            if (558.0..665.0).contains(&x) {
                self.vm.move_gallery_focus(-16).map_err(vm_error)?;
            } else if (700.0..810.0).contains(&x) {
                self.vm.move_gallery_focus(16).map_err(vm_error)?;
            } else if (830.0..906.0).contains(&x) {
                self.vm
                    .set_gallery_page(MusicaSystemPage::Memories, 2)
                    .map_err(vm_error)?;
            }
        }
        Ok(())
    }
}
