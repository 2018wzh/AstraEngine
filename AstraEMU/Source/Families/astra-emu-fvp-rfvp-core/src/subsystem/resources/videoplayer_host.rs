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
pub enum HostMovieCommand {
    Play {
        resource_uri: String,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
    },
    Stop,
}

#[derive(Debug, Default)]
pub struct VideoPlayerManager {
    playing: bool,
    loaded: bool,
    modal: bool,
    pending_commands: Vec<HostMovieCommand>,
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
        let name = movie_path
            .as_ref()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("RFVP_MOVIE_RESOURCE_URI_NOT_UTF8"))?
            .to_owned();
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
        _bytes: Vec<u8>,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
        _motion: &mut MotionManager,
        _audio_manager: Option<Arc<AudioManager>>,
    ) -> Result<()> {
        self.playing = true;
        self.loaded = true;
        self.modal = matches!(mode, MovieMode::ModalWithAudio);
        self.pending_commands.push(HostMovieCommand::Play {
            resource_uri: movie_name.to_string(),
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
        if self.playing {
            self.pending_commands.push(HostMovieCommand::Stop);
        }
        self.playing = false;
        self.modal = false;
    }

    pub fn complete(&mut self, _motion: &mut MotionManager) {
        self.playing = false;
        self.modal = false;
        self.loaded = false;
    }

    pub fn restore_pending(
        &mut self,
        resource_uri: String,
        mode: MovieMode,
        screen_w: u32,
        screen_h: u32,
    ) -> Result<()> {
        if self.playing || !self.pending_commands.is_empty() {
            anyhow::bail!("RFVP_MOVIE_RESTORE_CONFLICT");
        }
        self.playing = true;
        self.loaded = true;
        self.modal = matches!(mode, MovieMode::ModalWithAudio);
        self.pending_commands.push(HostMovieCommand::Play {
            resource_uri,
            mode,
            screen_w,
            screen_h,
        });
        Ok(())
    }

    pub fn drain_host_commands(&mut self, out: &mut Vec<HostMovieCommand>) {
        out.extend(self.pending_commands.drain(..));
    }
}
