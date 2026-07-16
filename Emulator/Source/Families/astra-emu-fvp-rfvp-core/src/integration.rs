use std::io::{Read, Write};
use std::sync::Arc;

use anyhow::{Context, Result};
use bincode::Options;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};

use crate::{
    host_api::{
        BlendMode, DrawSolidCommand, DrawSpriteCommand, RectI32, RfvpError, RfvpRenderer,
        TextureDesc, TextureFilter, TextureId, TextureRect, Vertex2D,
    },
    rendering::prim_commands::{render_motion_to_host, HostPrimRenderCache},
    script::{
        global::{Global, GLOBAL},
        parser::{Nls, Parser},
        Variant,
    },
    subsystem::{
        resources::{input_manager::KeyCode, thread_manager::ThreadManager},
        save_state::SaveStateSnapshotV1,
        world::{GameData, RuntimeVfs, SyscallJournalEntry},
    },
    vm_runner::{VmRunner, VmTraceRecord},
};

const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SNAPSHOT_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const SNAPSHOT_ZLIB_MAGIC: &[u8; 8] = b"AFVPSZ02";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshotV1 {
    pub version: u16,
    pub save_state: SaveStateSnapshotV1,
    pub globals_volatile: Vec<Variant>,
    pub non_volatile_count: u16,
    pub volatile_count: u16,
}

pub struct RuntimeSession {
    parser: Parser,
    game: GameData,
    runner: VmRunner,
    globals_non_volatile: Vec<Variant>,
    globals_volatile: Vec<Variant>,
    non_volatile_count: u16,
    volatile_count: u16,
    render_cache: HostPrimRenderCache,
}

#[derive(Debug, Clone)]
pub struct RecordedTextureUpdate {
    pub texture_id: u32,
    pub desc: TextureDesc,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RecordedDraw {
    pub texture_id: u32,
    pub vertices: [Vertex2D; 4],
    pub blend: BlendMode,
    pub scissor: Option<RectI32>,
}

#[derive(Debug, Clone)]
pub struct RecordedRenderFrame {
    pub width: u32,
    pub height: u32,
    pub texture_updates: Vec<RecordedTextureUpdate>,
    pub draws: Vec<RecordedDraw>,
}

#[derive(Default)]
struct RecordingRenderer {
    width: u32,
    height: u32,
    texture_updates: Vec<RecordedTextureUpdate>,
    draws: Vec<RecordedDraw>,
}

impl RfvpRenderer for RecordingRenderer {
    fn create_texture(
        &mut self,
        id: TextureId,
        desc: TextureDesc,
        pixels: Option<&[u8]>,
    ) -> crate::host_api::RfvpResult<()> {
        let pixels = pixels.ok_or(RfvpError::InvalidData)?;
        self.texture_updates.push(RecordedTextureUpdate {
            texture_id: id.0,
            desc,
            pixels: pixels.to_vec(),
        });
        Ok(())
    }

    fn update_texture(
        &mut self,
        _id: TextureId,
        _rect: TextureRect,
        _pixels: &[u8],
    ) -> crate::host_api::RfvpResult<()> {
        Err(RfvpError::Unsupported)
    }

    fn destroy_texture(&mut self, _id: TextureId) {}

    fn begin_frame(
        &mut self,
        width: u32,
        height: u32,
        _clear: Option<crate::host_api::ColorRgba>,
    ) -> crate::host_api::RfvpResult<()> {
        self.width = width;
        self.height = height;
        self.draws.clear();
        Ok(())
    }

    fn draw_sprite(&mut self, command: &DrawSpriteCommand) -> crate::host_api::RfvpResult<()> {
        if command.filter != TextureFilter::Linear {
            return Err(RfvpError::Unsupported);
        }
        self.draws.push(RecordedDraw {
            texture_id: command.texture.0,
            vertices: command.vertices,
            blend: command.blend,
            scissor: command.scissor,
        });
        Ok(())
    }

    fn draw_solid(&mut self, _command: &DrawSolidCommand) -> crate::host_api::RfvpResult<()> {
        Err(RfvpError::Unsupported)
    }

    fn end_frame(&mut self) -> crate::host_api::RfvpResult<()> {
        Ok(())
    }

    fn present(&mut self) -> crate::host_api::RfvpResult<()> {
        Ok(())
    }
}

impl RuntimeSession {
    pub fn read_vfs_file(&self, resource_uri: &str) -> Result<Vec<u8>> {
        self.game
            .vfs_load_file(resource_uri)
            .with_context(|| "resolve FVP session resource")
    }

    pub fn new(
        script: Vec<u8>,
        nls: Nls,
        vfs: Arc<dyn RuntimeVfs>,
        stage_dimensions: (u32, u32),
    ) -> Result<Self> {
        let parser = Parser::from_bytes(script, nls).context("parse FVP HCB")?;
        let non_volatile_count = parser.get_non_volatile_global_count();
        let volatile_count = parser.get_volatile_global_count();
        {
            let mut globals = GLOBAL
                .lock()
                .map_err(|_| anyhow::anyhow!("RFVP_GLOBAL_LOCK_POISONED"))?;
            *globals = Global::new();
            globals.init_with(non_volatile_count, volatile_count);
        }
        let mut game = GameData::default();
        game.set_window(crate::subsystem::resources::window::Window::new(
            stage_dimensions,
            1.0,
        ));
        game.set_runtime_vfs(vfs);
        let mut runner = VmRunner::new(ThreadManager::new());
        runner.start_main(parser.get_entry_point());
        Ok(Self {
            parser,
            game,
            runner,
            globals_non_volatile: vec![Variant::Nil; non_volatile_count as usize],
            globals_volatile: vec![Variant::Nil; volatile_count as usize],
            non_volatile_count,
            volatile_count,
            render_cache: HostPrimRenderCache::new(),
        })
    }

    pub fn tick_bounded(&mut self, frame_time_ms: u64, max_instructions: u64) -> Result<()> {
        self.activate_globals()?;
        let result = self.runner.tick_bounded(
            &mut self.game,
            &mut self.parser,
            frame_time_ms,
            max_instructions,
        );
        self.capture_globals()?;
        result.map(|_| ())
    }

    pub fn inject_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        self.game.inject_keycode(key, pressed, repeat);
    }

    pub fn inject_pointer(&mut self, x: i32, y: i32, in_screen: bool) {
        self.game.inject_pointer_position(x, y, in_screen);
    }

    pub fn inject_wheel(&mut self, value: i32) {
        self.game.inject_wheel(value);
    }

    pub fn take_trace(&mut self) -> Vec<VmTraceRecord> {
        self.runner.take_trace()
    }

    pub fn take_syscall_journal(&mut self) -> Vec<SyscallJournalEntry> {
        self.game.take_syscall_journal()
    }

    #[cfg(feature = "host-command-audio")]
    pub fn take_audio_commands(&mut self) -> Vec<crate::rfvp_audio::AudioCommand> {
        let mut commands = Vec::new();
        self.game.audio_manager().drain_commands(&mut commands);
        commands
    }

    #[cfg(feature = "host-command-video")]
    pub fn take_movie_commands(
        &mut self,
    ) -> Vec<crate::subsystem::resources::videoplayer::HostMovieCommand> {
        let mut commands = Vec::new();
        self.game.video_manager.drain_host_commands(&mut commands);
        commands
    }

    #[cfg(feature = "host-command-video")]
    pub fn complete_movie(&mut self) {
        self.game
            .video_manager
            .complete(&mut self.game.motion_manager);
        self.game.set_halt(false);
    }

    #[cfg(feature = "host-command-video")]
    pub fn restore_pending_movie(
        &mut self,
        resource_uri: String,
        mode: crate::subsystem::resources::videoplayer::MovieMode,
        screen_w: u32,
        screen_h: u32,
    ) -> Result<()> {
        self.game
            .video_manager
            .restore_pending(resource_uri, mode, screen_w, screen_h)?;
        self.game.set_halt(true);
        Ok(())
    }

    pub fn record_render_frame(&mut self) -> Result<RecordedRenderFrame> {
        let dimensions = self.game.window_ref().dimensions();
        let mut recorder = RecordingRenderer::default();
        render_motion_to_host(
            &mut recorder,
            &mut self.render_cache,
            &self.game.motion_manager,
            dimensions,
        )
        .map_err(|error| anyhow::anyhow!("RFVP_RENDER_RECORD:{error:?}"))?;
        Ok(RecordedRenderFrame {
            width: recorder.width,
            height: recorder.height,
            texture_updates: recorder.texture_updates,
            draws: recorder.draws,
        })
    }

    pub fn is_terminal(&self) -> bool {
        self.runner.thread_manager().get_should_break()
            || self.game.get_game_should_exit()
            || self.game.get_main_thread_exited()
    }

    pub fn has_pending_wait(&self) -> bool {
        let manager = self.runner.thread_manager();
        self.game.get_halt()
            || (0..manager.total_contexts()).any(|id| {
                let status = manager.get_context_status(id as u32);
                status.intersects(
                    crate::script::context::ThreadState::CONTEXT_STATUS_WAIT
                        | crate::script::context::ThreadState::CONTEXT_STATUS_SLEEP
                        | crate::script::context::ThreadState::CONTEXT_STATUS_TEXT
                        | crate::script::context::ThreadState::CONTEXT_STATUS_DISSOLVE_WAIT,
                )
            })
    }

    pub fn has_text_wait(&self) -> bool {
        let manager = self.runner.thread_manager();
        (0..manager.total_contexts()).any(|id| {
            manager
                .get_context_status(id as u32)
                .contains(crate::script::context::ThreadState::CONTEXT_STATUS_TEXT)
        })
    }

    pub fn snapshot(&mut self) -> Result<Vec<u8>> {
        self.activate_globals()?;
        let snapshot = RuntimeSnapshotV1 {
            version: 1,
            save_state: SaveStateSnapshotV1::capture_with_thread_manager(
                &mut self.game,
                self.runner.thread_manager(),
            ),
            globals_volatile: self.globals_volatile.clone(),
            non_volatile_count: self.non_volatile_count,
            volatile_count: self.volatile_count,
        };
        let bytes = bincode_options(MAX_SNAPSHOT_UNCOMPRESSED_BYTES)
            .serialize(&snapshot)
            .context("serialize FVP runtime snapshot")?;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&bytes)
            .context("compress FVP runtime snapshot")?;
        let compressed = encoder.finish().context("finish FVP runtime snapshot")?;
        let mut envelope = Vec::with_capacity(16 + compressed.len());
        envelope.extend_from_slice(SNAPSHOT_ZLIB_MAGIC);
        envelope.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        envelope.extend_from_slice(&compressed);
        if envelope.len() as u64 > MAX_SNAPSHOT_BYTES {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_TOO_LARGE");
        }
        Ok(envelope)
    }

    pub fn restore(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_TOO_LARGE");
        }
        let snapshot = decode_runtime_snapshot(bytes)?;
        if snapshot.version != 1
            || snapshot.non_volatile_count != self.non_volatile_count
            || snapshot.volatile_count != self.volatile_count
        {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_IDENTITY_MISMATCH");
        }
        self.globals_non_volatile = snapshot.save_state.globals_non_volatile.clone();
        self.globals_volatile = snapshot.globals_volatile;
        self.activate_globals()?;
        snapshot
            .save_state
            .apply(&mut self.game, self.runner.thread_manager_mut())?;
        self.game.take_syscall_journal();
        self.runner.take_trace();
        self.render_cache = HostPrimRenderCache::new();
        Ok(())
    }

    pub fn state_bytes(&mut self) -> Result<Vec<u8>> {
        self.snapshot()
    }

    fn activate_globals(&self) -> Result<()> {
        let mut globals = GLOBAL
            .lock()
            .map_err(|_| anyhow::anyhow!("RFVP_GLOBAL_LOCK_POISONED"))?;
        *globals = Global::new();
        globals.init_with(self.non_volatile_count, self.volatile_count);
        globals.restore_non_volatile(&self.globals_non_volatile);
        globals.restore_volatile_globals(
            self.non_volatile_count,
            self.volatile_count,
            &self.globals_volatile,
        );
        Ok(())
    }

    fn capture_globals(&mut self) -> Result<()> {
        let globals = GLOBAL
            .lock()
            .map_err(|_| anyhow::anyhow!("RFVP_GLOBAL_LOCK_POISONED"))?;
        self.globals_non_volatile = globals.snapshot_non_volatile();
        self.globals_volatile = globals.snapshot_volatile_globals();
        Ok(())
    }
}

pub fn decode_runtime_snapshot(bytes: &[u8]) -> Result<RuntimeSnapshotV1> {
    let decoded = if bytes.starts_with(SNAPSHOT_ZLIB_MAGIC) {
        if bytes.len() < 16 {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_ENVELOPE_TRUNCATED");
        }
        let declared = u64::from_le_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| anyhow::anyhow!("RFVP_RUNTIME_SNAPSHOT_ENVELOPE_TRUNCATED"))?,
        );
        if declared > MAX_SNAPSHOT_UNCOMPRESSED_BYTES {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_UNCOMPRESSED_TOO_LARGE");
        }
        let decoder = ZlibDecoder::new(&bytes[16..]);
        let mut decoded = Vec::with_capacity(declared as usize);
        decoder
            .take(declared.saturating_add(1))
            .read_to_end(&mut decoded)
            .context("decompress FVP runtime snapshot")?;
        if decoded.len() as u64 != declared {
            anyhow::bail!("RFVP_RUNTIME_SNAPSHOT_UNCOMPRESSED_SIZE_MISMATCH");
        }
        decoded
    } else {
        bytes.to_vec()
    };
    bincode_options(MAX_SNAPSHOT_UNCOMPRESSED_BYTES)
        .deserialize(&decoded)
        .context("decode FVP runtime snapshot")
}

fn bincode_options(limit: u64) -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(limit)
        .reject_trailing_bytes()
}
