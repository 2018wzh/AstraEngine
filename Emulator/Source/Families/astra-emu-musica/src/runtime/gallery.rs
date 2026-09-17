use super::*;
const TRACKS: [u32; 47] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 31, 32,
    33, 34, 35, 36, 37, 38, 41, 42, 43, 51, 52, 61, 62, 71, 72, 73, 74, 81, 82, 83, 84,
];
pub(crate) const REPLAYS: [&str; 4] = [
    "fb_ren_04.sc",
    "fb_aya_04.sc",
    "fb_sui_04.sc",
    "fb_tou_04.sc",
];
pub(crate) const MOVIES: [&str; 4] = [
    "fb_aya_12.sc",
    "fb_ren_16.sc",
    "fb_sui_12.sc",
    "fb_tou_12.sc",
];
pub(crate) const MOVIE_LABELS: [&str; 4] =
    ["ed_ayame.avi", "ed_ren.avi", "ed_sui.avi", "ed_tohka.avi"];
pub(crate) fn count(page: &MusicaSystemPage) -> Option<u32> {
    match page {
        MusicaSystemPage::Memories => Some(5),
        MusicaSystemPage::GalleryCg => Some(12),
        MusicaSystemPage::GalleryBgm => Some(47),
        MusicaSystemPage::GalleryReplay | MusicaSystemPage::GalleryMovie => Some(4),
        _ => None,
    }
}

pub(crate) fn bgm_uri(focus: u32) -> Result<String, MusicaRuntimeError> {
    let track = TRACKS
        .get(focus as usize)
        .ok_or(MusicaRuntimeError::State)?;
    Ok(format!("musica:/bgm/BGM{track:03}.ogg"))
}
pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if let Some(count) = count(&state.system_ui.page) {
        if state.launch_mode != MusicaLaunchMode::Title
            || state.wait.is_some()
            || state.terminal
            || state.system_ui.focus_index >= count
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}
impl MusicaVm {
    pub(crate) fn set_gallery_page(
        &mut self,
        page: MusicaSystemPage,
        focus: u32,
    ) -> Result<(), MusicaRuntimeError> {
        if self.state.launch_mode != MusicaLaunchMode::Title
            || self.state.wait.is_some()
            || self.state.terminal
            || !(self.state.system_ui.page == MusicaSystemPage::Title
                || count(&self.state.system_ui.page).is_some())
        {
            return Err(MusicaRuntimeError::State);
        }
        if page == MusicaSystemPage::Title {
            if focus as usize >= super::title::rows(self.title_variant()).len() {
                return Err(MusicaRuntimeError::State);
            }
        } else if focus >= count(&page).ok_or(MusicaRuntimeError::State)?
            || self.title_variant() != 2
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.page = page;
        self.state.system_ui.focus_index = focus;
        Ok(())
    }
    pub(crate) fn move_gallery_focus(&mut self, delta: i32) -> Result<(), MusicaRuntimeError> {
        let count = count(&self.state.system_ui.page).ok_or(MusicaRuntimeError::State)?;
        let focus = (i64::from(self.state.system_ui.focus_index) + i64::from(delta))
            .rem_euclid(i64::from(count)) as u32;
        self.set_gallery_page(self.state.system_ui.page.clone(), focus)
    }
    pub(crate) fn gallery_bgm_play(
        &mut self,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::GalleryBgm {
            return Err(MusicaRuntimeError::State);
        }
        let uri = bgm_uri(self.state.system_ui.focus_index)?;
        let mut commands = Vec::new();
        if self.state.audio.get(&0).is_some_and(|a| a.playing) {
            commands.push(MusicaAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id: 0,
                fade_ms: 0,
            });
        }
        audio_commands::append_audio_load_and_play(
            &mut self.state,
            &mut commands,
            0,
            &uri,
            1000,
            0,
            true,
            0,
        )?;
        self.state.audio.insert(
            0,
            MusicaAudioState {
                bus: "bgm".into(),
                resource_uri: uri,
                looped: true,
                volume_milli: 1000,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        Ok(commands)
    }
    pub(crate) fn gallery_bgm_stop(
        &mut self,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MusicaSystemPage::GalleryBgm | MusicaSystemPage::Memories
        ) {
            return Err(MusicaRuntimeError::State);
        }
        match audio_commands::stop_audio_stream(&mut self.state, 0, 0)? {
            Some(MusicaVmEvent::Audio { commands }) => Ok(commands),
            None => Ok(Vec::new()),
            _ => Err(MusicaRuntimeError::State),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_snapshot_focus_and_audio_keep_native_bounds() {
        let source = b".end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        assert!(vm.set_gallery_page(MusicaSystemPage::Memories, 0).is_err());
        vm.begin_title_launch().unwrap();
        assert!(vm.set_gallery_page(MusicaSystemPage::Memories, 0).is_err());
        vm.merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();
        for page in [
            MusicaSystemPage::Memories,
            MusicaSystemPage::GalleryCg,
            MusicaSystemPage::GalleryBgm,
            MusicaSystemPage::GalleryReplay,
            MusicaSystemPage::GalleryMovie,
        ] {
            let last = count(&page).unwrap() - 1;
            vm.set_gallery_page(page.clone(), last).unwrap();
            let saved = vm.encode_native_save().unwrap();
            vm.restore_native_save(&saved, 1).unwrap();
            assert_eq!(vm.state.system_ui.page, page);
            assert_eq!(vm.state.system_ui.focus_index, last);
            assert!(vm.set_gallery_page(page, last + 1).is_err());
            let mut corrupt = vm.state.clone();
            corrupt.system_ui.focus_index = last + 1;
            assert!(
                MusicaVm::decode_native_save(&postcard::to_allocvec(&corrupt).unwrap()).is_err()
            );
        }
        vm.set_gallery_page(MusicaSystemPage::GalleryBgm, 0)
            .unwrap();
        vm.move_gallery_focus(-1).unwrap();
        assert_eq!(vm.state.system_ui.focus_index, 46);
        assert_eq!(bgm_uri(24).unwrap(), "musica:/bgm/BGM031.ogg");
        assert!(bgm_uri(47).is_err());
        assert_eq!(vm.gallery_bgm_play().unwrap().len(), 2);
        assert_eq!(vm.gallery_bgm_play().unwrap().len(), 3);
        assert_eq!(vm.gallery_bgm_stop().unwrap().len(), 1);
        assert!(vm.gallery_bgm_stop().unwrap().is_empty());
        vm.set_gallery_page(MusicaSystemPage::Title, 0).unwrap();
        assert!(vm.gallery_bgm_play().is_err());
    }
}
