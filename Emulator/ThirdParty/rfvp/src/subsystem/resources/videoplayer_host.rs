use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::rfvp_audio::AudioManager;

use super::motion_manager::MotionManager;

pub const MOVIE_GRAPH_ID: u16 = 4063;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieMode {
    ModalWithAudio,
    LayerNoAudio,
}

#[derive(Debug, Clone)]
pub struct HostMovieCommand {
    pub resource_uri: String,
    pub byte_len: u64,
    pub mode: MovieMode,
    pub screen_w: u32,
    pub screen_h: u32,
}

#[derive(Debug, Default)]
pub struct VideoPlayerManager {
    playing: bool,
    loaded: bool,
    modal: bool,
    pending_commands: Vec<HostMovieCommand>,
    pending_stop: bool,
}

impl VideoPlayerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn is_modal_active(&self) -> bool {
        self.playing && self.modal
    }

    pub fn start(
        &mut self,
        movie_path: impl AsRef<Path>,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
        motion: &mut MotionManager,
        audio_manager: Option<Arc<AudioManager>>,
    ) -> Result<()> {
        let name = movie_path.as_ref().to_string_lossy().to_string();
        self.start_from_bytes(
            &name,
            Vec::new(),
            mode,
            screen_w,
            screen_h,
            motion,
            audio_manager,
        )
    }

    pub fn start_from_bytes(
        &mut self,
        movie_name: &str,
        bytes: Vec<u8>,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
        _motion: &mut MotionManager,
        _audio_manager: Option<Arc<AudioManager>>,
    ) -> Result<()> {
        self.playing = true;
        self.loaded = true;
        self.modal = matches!(mode, MovieMode::ModalWithAudio);
        self.pending_commands.push(HostMovieCommand {
            resource_uri: movie_name.to_string(),
            byte_len: u64::try_from(bytes.len())
                .map_err(|_| anyhow::anyhow!("movie bytes exceed u64"))?,
            mode,
            screen_w,
            screen_h,
        });
        Ok(())
    }

    /// Queue a host-owned VFS resource. Hosted sessions must not copy an
    /// encoded movie into the core just to request playback.
    pub fn start_from_resource(
        &mut self,
        resource_uri: &str,
        byte_len: u64,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
        _motion: &mut MotionManager,
        _audio_manager: Option<Arc<AudioManager>>,
    ) -> Result<()> {
        if resource_uri.is_empty() || byte_len == 0 {
            anyhow::bail!("hosted movie resource is empty");
        }
        self.playing = true;
        self.loaded = true;
        self.modal = matches!(mode, MovieMode::ModalWithAudio);
        self.pending_commands.push(HostMovieCommand {
            resource_uri: resource_uri.to_string(),
            byte_len,
            mode,
            screen_w,
            screen_h,
        });
        Ok(())
    }

    pub fn tick(&mut self, _motion: &mut MotionManager) -> Result<()> {
        Ok(())
    }

    pub fn stop(&mut self, _motion: &mut MotionManager) {
        self.stop_inner(true);
    }

    /// Stop a host-owned movie after the embedding has already consumed its
    /// playback completion. This clears the core state without emitting a
    /// second host operation.
    pub fn stop_without_host_command(&mut self, _motion: &mut MotionManager) {
        self.stop_inner(false);
    }

    fn stop_inner(&mut self, notify_host: bool) {
        if notify_host && self.playing {
            self.pending_stop = true;
        }
        self.playing = false;
        self.loaded = false;
        self.modal = false;
    }

    pub fn drain_host_commands(&mut self, out: &mut Vec<HostMovieCommand>) {
        out.extend(self.pending_commands.drain(..));
    }

    pub fn take_host_stop_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_stop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_stop_emits_a_host_stop_request() {
        let mut manager = VideoPlayerManager::new();
        let mut motion = MotionManager::new();
        manager
            .start_from_resource(
                "movie.wmv",
                1,
                MovieMode::ModalWithAudio,
                800,
                600,
                &mut motion,
                None,
            )
            .expect("movie request is queued");

        manager.stop(&mut motion);

        let mut commands = Vec::new();
        manager.drain_host_commands(&mut commands);
        assert_eq!(commands.len(), 1);
        assert!(manager.take_host_stop_request());
        assert!(!manager.take_host_stop_request());
    }
}
