use alloc::boxed::Box;
#[cfg(not(feature = "old_school"))]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(feature = "hosted")]
use bincode::Options;
#[cfg(feature = "hosted")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "hosted")]
use std::io::Read;

#[cfg(feature = "old_school")]
use crate::font::Font;
use crate::host_api::{
    AudioStreamId, HitProxyTable, RfvpAudio, RfvpClock, RfvpError, RfvpEvent, RfvpFile,
    RfvpFileInfo, RfvpFileSystem, RfvpHost, RfvpLogLevel, RfvpResult,
};
#[cfg(feature = "old_school")]
use crate::host_api::{FatalErrorCode, PlatformCallbacks};
use crate::rendering::prim_commands::{render_motion_to_host, HostPrimRenderCache};
#[cfg(not(feature = "hosted"))]
use crate::script::global::GLOBAL;
use crate::script::parser::{Nls, Parser};
#[cfg(feature = "hosted")]
use crate::soft_render::SoftRenderer;
use crate::subsystem::anzu_scene::AnzuScene;
#[cfg(feature = "hosted")]
use crate::subsystem::global_savedata::GlobalSaveDataV1;
#[cfg(feature = "hosted")]
use crate::subsystem::resources::save_manager::{HostedSaveFileOperation, SaveItem};
use crate::subsystem::resources::text_manager::FontEnumerator;
use crate::subsystem::resources::vfs::Vfs;
use crate::subsystem::resources::window::Window;
#[cfg(feature = "hosted")]
use crate::subsystem::resources::{
    input_manager::InputManagerSnapshotV1, motion_manager::MotionManagerCanonicalStateV1,
    thread_manager::ThreadManagerSnapshotV1, thread_wrapper::ThreadWrapperSnapshotV1,
    time::TimeSnapshotV1, timer_manager::TimerManagerSnapshotV1,
};
#[cfg(feature = "hosted")]
use crate::subsystem::save_state::{
    try_decode_state_chunk_v1, AudioSnapshotV1, SaveStateSnapshotV1,
};
use crate::subsystem::world::GameData;
#[cfg(feature = "hosted")]
use crate::subsystem::world::RuntimeGameStateSnapshotV1;
#[cfg(feature = "hosted")]
use crate::vm_runner::HostedVmTraceRecord;
use crate::vm_runner::VmRunner;
#[cfg(feature = "old_school")]
use core_maths::CoreFloat;

#[cfg(feature = "old_school")]
const MISSING_OLD_SCHOOL_FONT_MESSAGE: &str =
    "Required font file defualt.tmap was not found in the game directory.";
#[cfg(feature = "old_school")]
const INVALID_OLD_SCHOOL_FONT_MESSAGE: &str =
    "Required font file defualt.tmap exists but is not a valid bitmap font.";
#[cfg(feature = "old_school")]
const MISSING_OLD_SCHOOL_CONFIG_MESSAGE: &str =
    "Required old-school config file rfvp.toml was not found in the game directory.";
#[cfg(feature = "old_school")]
const INVALID_OLD_SCHOOL_CONFIG_MESSAGE: &str =
    "Required old-school config file rfvp.toml is invalid: expected exactly one top-level float field, scale.";

fn hosted_keycode(
    key: crate::host_api::KeyCode,
) -> Option<crate::subsystem::resources::input_manager::KeyCode> {
    use crate::host_api::KeyCode as HostKeyCode;
    use crate::subsystem::resources::input_manager::KeyCode as InputKeyCode;

    Some(match key {
        HostKeyCode::Escape => InputKeyCode::Esc,
        HostKeyCode::Return => InputKeyCode::Enter,
        HostKeyCode::Space => InputKeyCode::Space,
        HostKeyCode::Tab => InputKeyCode::Tab,
        HostKeyCode::Left => InputKeyCode::LeftArrow,
        HostKeyCode::Right => InputKeyCode::RightArrow,
        HostKeyCode::Up => InputKeyCode::UpArrow,
        HostKeyCode::Down => InputKeyCode::DownArrow,
        HostKeyCode::Shift => InputKeyCode::Shift,
        HostKeyCode::Control => InputKeyCode::Ctrl,
        HostKeyCode::Function(number @ 1..=12) => match number {
            1 => InputKeyCode::F1,
            2 => InputKeyCode::F2,
            3 => InputKeyCode::F3,
            4 => InputKeyCode::F4,
            5 => InputKeyCode::F5,
            6 => InputKeyCode::F6,
            7 => InputKeyCode::F7,
            8 => InputKeyCode::F8,
            9 => InputKeyCode::F9,
            10 => InputKeyCode::F10,
            11 => InputKeyCode::F11,
            12 => InputKeyCode::F12,
            _ => unreachable!("function key range is bounded above"),
        },
        HostKeyCode::Backspace
        | HostKeyCode::PageUp
        | HostKeyCode::PageDown
        | HostKeyCode::Home
        | HostKeyCode::End
        | HostKeyCode::Insert
        | HostKeyCode::Delete
        | HostKeyCode::Alt
        | HostKeyCode::Character(_)
        | HostKeyCode::Function(_)
        | HostKeyCode::Unknown(_) => return None,
    })
}

#[cfg(test)]
mod hosted_input_tests {
    use super::{hosted_keycode, RfvpCore, RfvpCoreConfig};
    use crate::host_api::{InputModifiers, KeyCode as HostKeyCode, RfvpEvent};
    use crate::subsystem::resources::input_manager::KeyCode as InputKeyCode;

    #[test]
    fn maps_hosted_confirm_and_navigation_keys() {
        assert_eq!(
            hosted_keycode(HostKeyCode::Return),
            Some(InputKeyCode::Enter)
        );
        assert_eq!(
            hosted_keycode(HostKeyCode::Left),
            Some(InputKeyCode::LeftArrow)
        );
        assert_eq!(
            hosted_keycode(HostKeyCode::Function(12)),
            Some(InputKeyCode::F12)
        );
    }

    #[test]
    fn rejects_hosted_keys_without_an_fvp_input_bit() {
        assert_eq!(hosted_keycode(HostKeyCode::Character('x')), None);
        assert_eq!(hosted_keycode(HostKeyCode::Function(13)), None);
    }

    #[test]
    fn hosted_event_is_latched_exactly_once_before_vm() {
        let mut core = RfvpCore::new(RfvpCoreConfig::default());
        core.push_event(RfvpEvent::KeyDown {
            key: HostKeyCode::Return,
            repeat: false,
            modifiers: InputModifiers::empty(),
        })
        .unwrap();

        core.apply_pending_events_to_game_data();
        assert_eq!(core.game_data.inputs_manager.get_input_down(), 0);

        core.game_data.inputs_manager.begin_frame();
        assert_ne!(core.game_data.inputs_manager.get_input_down(), 0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RfvpCoreConfig {
    pub virtual_width: u32,
    pub virtual_height: u32,
    pub max_pending_events: usize,
}

impl Default for RfvpCoreConfig {
    fn default() -> Self {
        Self {
            virtual_width: 800,
            virtual_height: 600,
            max_pending_events: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RfvpBootConfig<'a> {
    pub asset_root: &'a str,
    pub hcb_extension: &'a str,
    pub max_hcb_bytes: usize,
    pub max_manifest_entries: usize,
    pub nls: Nls,
}

impl<'a> Default for RfvpBootConfig<'a> {
    fn default() -> Self {
        Self {
            asset_root: ".",
            hcb_extension: "hcb",
            max_hcb_bytes: 64 * 1024 * 1024,
            max_manifest_entries: 1024,
            nls: Nls::ShiftJIS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RfvpTickResult {
    pub frame_index: u64,
    pub consumed_events: usize,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfvpCoreRunState {
    NotBooted,
    Booted,
    BootFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfvpResourceEntry {
    pub path: String,
    pub info: RfvpFileInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfvpLoadedGame {
    pub asset_root: String,
    pub hcb_path: String,
    pub hcb_bytes: Vec<u8>,
    pub hcb_info: RfvpFileInfo,
    pub hcb_manifest: Vec<RfvpResourceEntry>,
}

/// In-memory exact checkpoint for a hosted session.  It contains no host
/// handles or platform paths and is valid only for the already-booted session
/// with the same loaded game and resource binding.
#[cfg(feature = "hosted")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedCoreSnapshot {
    pub version: u16,
    pub frame_index: u64,
    pub last_tick_us: Option<u64>,
    pub last_dissolve_transitioning: bool,
    pub last_dissolve2_transitioning: bool,
    pub quit_requested: bool,
    pub save_state: SaveStateSnapshotV1,
    pub globals: crate::script::global::HostedGlobalSnapshot,
    pub input: InputManagerSnapshotV1,
    pub timers: TimerManagerSnapshotV1,
    pub time: TimeSnapshotV1,
    pub deferred_threads: ThreadWrapperSnapshotV1,
    pub runtime_state: RuntimeGameStateSnapshotV1,
    pub global_state: GlobalSaveDataV1,
}

#[cfg(feature = "hosted")]
pub const HOSTED_CORE_SNAPSHOT_VERSION: u16 = 3;

/// Stable semantic state used by an embedding for verification.  This is not
/// a persistence format: it deliberately represents graphics pixels by
/// content digest so image allocation and decode-cache layout cannot perturb
/// the state identity after a restore.
#[cfg(feature = "hosted")]
#[derive(Debug, Clone, Serialize)]
struct HostedCanonicalStateV1 {
    version: u16,
    frame_index: u64,
    last_tick_us: Option<u64>,
    last_dissolve_transitioning: bool,
    last_dissolve2_transitioning: bool,
    quit_requested: bool,
    globals: crate::script::global::HostedGlobalSnapshot,
    input: InputManagerSnapshotV1,
    timers: TimerManagerSnapshotV1,
    time: TimeSnapshotV1,
    deferred_threads: ThreadWrapperSnapshotV1,
    runtime_state: RuntimeGameStateSnapshotV1,
    global_state: GlobalSaveDataV1,
    motion: MotionManagerCanonicalStateV1,
    audio: AudioSnapshotV1,
    vm: ThreadManagerSnapshotV1,
}

/// Digest-only hosted state breakdown for restore diagnostics.  It exposes no
/// script payload, resource bytes or host paths.
#[cfg(feature = "hosted")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedStateComponentHashesV1 {
    pub session: [u8; 32],
    pub globals: [u8; 32],
    pub input_and_time: [u8; 32],
    pub runtime: [u8; 32],
    pub motion: [u8; 32],
    pub audio: [u8; 32],
    pub vm: [u8; 32],
}

#[cfg(feature = "hosted")]
fn hosted_component_hash<T: Serialize>(value: &T) -> RfvpResult<[u8; 32]> {
    let bytes = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(value)
        .map_err(|_| RfvpError::InvalidData)?;
    use sha2::Digest;
    Ok(sha2::Sha256::digest(bytes).into())
}

pub struct RfvpCore {
    config: RfvpCoreConfig,
    pending_events: Vec<RfvpEvent>,
    frame_index: u64,
    last_tick_us: Option<u64>,
    last_dissolve_transitioning: bool,
    last_dissolve2_transitioning: bool,
    quit_requested: bool,
    run_state: RfvpCoreRunState,
    loaded_game: Option<RfvpLoadedGame>,
    parser: Option<Parser>,
    game_data: GameData,
    vm_runner: Option<VmRunner>,
    #[cfg(feature = "hosted")]
    hosted_trace_capacity: usize,
    render_cache: HostPrimRenderCache,
    hit_proxies: HitProxyTable,
    last_error: Option<RfvpError>,
    last_error_detail: Option<String>,
}

impl RfvpCore {
    pub fn new(config: RfvpCoreConfig) -> Self {
        Self {
            config,
            pending_events: Vec::new(),
            frame_index: 0,
            last_tick_us: None,
            last_dissolve_transitioning: false,
            last_dissolve2_transitioning: false,
            quit_requested: false,
            run_state: RfvpCoreRunState::NotBooted,
            loaded_game: None,
            parser: None,
            game_data: GameData::default(),
            vm_runner: None,
            #[cfg(feature = "hosted")]
            hosted_trace_capacity: 0,
            render_cache: HostPrimRenderCache::new(),
            hit_proxies: HitProxyTable::default(),
            last_error: None,
            last_error_detail: None,
        }
    }

    pub fn config(&self) -> RfvpCoreConfig {
        self.config
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// A hosted session is terminal when either the host requested shutdown or
    /// the script's main execution path reached its terminal state.
    #[cfg(feature = "hosted")]
    pub fn hosted_terminal(&self) -> bool {
        self.quit_requested
            || self.game_data.get_main_thread_exited()
            || self.game_data.get_game_should_exit()
    }

    #[cfg(feature = "hosted")]
    pub(crate) fn hosted_visual_state(&self) -> crate::hosted::HostedVisualState {
        use crate::{
            hosted::{HostedDissolveKind, HostedVisualState},
            subsystem::resources::motion_manager::DissolveType,
        };

        let motion = &self.game_data.motion_manager;
        let dissolve_kind = match motion.get_dissolve_type() {
            DissolveType::None => HostedDissolveKind::None,
            DissolveType::Static => HostedDissolveKind::Static,
            DissolveType::ColoredFadeIn => HostedDissolveKind::ColoredFadeIn,
            DissolveType::ColoredFadeOut => HostedDissolveKind::ColoredFadeOut,
            DissolveType::MaskFadeIn => HostedDissolveKind::MaskFadeIn,
            DissolveType::MaskFadeInOut => HostedDissolveKind::MaskFadeInOut,
            DissolveType::MaskFadeOut => HostedDissolveKind::MaskFadeOut,
        };
        HostedVisualState {
            dissolve_kind,
            dissolve_color_id: motion.get_dissolve_color_id(),
            dissolve_alpha: motion.get_dissolve_alpha(),
            dissolve2_mode: motion.get_dissolve2_mode(),
            dissolve2_color_id: motion.get_dissolve2_color_id(),
            dissolve2_alpha: motion.get_dissolve2_alpha(),
            dissolve2_transitioning: motion.is_dissolve2_transitioning(),
        }
    }

    pub fn run_state(&self) -> RfvpCoreRunState {
        self.run_state
    }

    pub fn loaded_game(&self) -> Option<&RfvpLoadedGame> {
        self.loaded_game.as_ref()
    }

    pub fn last_error(&self) -> Option<RfvpError> {
        self.last_error
    }

    pub fn last_error_detail(&self) -> Option<&str> {
        self.last_error_detail.as_deref()
    }

    #[cfg(feature = "hosted")]
    pub fn set_hosted_trace_capacity(&mut self, capacity: usize) -> RfvpResult<()> {
        if let Some(vm_runner) = self.vm_runner.as_mut() {
            vm_runner
                .set_hosted_trace_capacity(capacity)
                .map_err(|_| RfvpError::CapacityExceeded)?;
        } else if capacity > 65_536 {
            return Err(RfvpError::CapacityExceeded);
        }
        self.hosted_trace_capacity = capacity;
        Ok(())
    }

    #[cfg(feature = "hosted")]
    pub fn hosted_trace(&self) -> RfvpResult<Vec<HostedVmTraceRecord>> {
        let vm_runner = self.vm_runner.as_ref().ok_or(RfvpError::InvalidData)?;
        Ok(vm_runner.hosted_trace())
    }

    #[cfg(feature = "hosted")]
    pub fn take_hosted_video_commands(
        &mut self,
    ) -> Vec<crate::subsystem::resources::videoplayer::HostMovieCommand> {
        let mut commands = Vec::new();
        self.game_data
            .video_manager
            .drain_host_commands(&mut commands);
        commands
    }

    #[cfg(feature = "hosted")]
    pub fn take_hosted_video_stop(&mut self) -> bool {
        self.game_data.video_manager.take_host_stop_request()
    }

    #[cfg(feature = "hosted")]
    pub(crate) fn take_hosted_text_events(
        &mut self,
    ) -> Vec<crate::subsystem::world::HostedTextEvent> {
        self.game_data.take_hosted_text_events()
    }

    #[cfg(feature = "hosted")]
    pub(crate) fn set_hosted_text_limits(&mut self, max_operations: usize, max_bytes: usize) {
        self.game_data
            .set_hosted_text_limits(max_operations, max_bytes);
    }

    /// Completes the single host-owned movie currently active in this
    /// session. The embedding must call this only after it has matched a
    /// previously emitted hosted video command; unsolicited completion is an
    /// embedding protocol error and must be rejected before this boundary.
    #[cfg(feature = "hosted")]
    pub fn complete_hosted_video(&mut self) -> RfvpResult<()> {
        if !self.game_data.video_manager.is_playing() {
            return Err(RfvpError::InvalidData);
        }
        self.game_data
            .video_manager
            .stop_without_host_command(&mut self.game_data.motion_manager);
        self.game_data.set_halt(false);
        Ok(())
    }

    #[cfg(feature = "hosted")]
    pub fn capture_hosted_snapshot(&self) -> RfvpResult<HostedCoreSnapshot> {
        if self.run_state != RfvpCoreRunState::Booted {
            return Err(RfvpError::InvalidData);
        }
        let vm_runner = self.vm_runner.as_ref().ok_or(RfvpError::InvalidData)?;
        Ok(HostedCoreSnapshot {
            version: HOSTED_CORE_SNAPSHOT_VERSION,
            frame_index: self.frame_index,
            last_tick_us: self.last_tick_us,
            last_dissolve_transitioning: self.last_dissolve_transitioning,
            last_dissolve2_transitioning: self.last_dissolve2_transitioning,
            quit_requested: self.quit_requested,
            save_state: SaveStateSnapshotV1::capture_hosted(
                &self.game_data,
                vm_runner.thread_manager(),
            ),
            globals: self.game_data.capture_hosted_globals(),
            input: self.game_data.inputs_manager.capture_snapshot_v1(),
            timers: self.game_data.timer_manager.capture_snapshot_v1(),
            time: self.game_data.time_ref().capture_snapshot_v1(),
            deferred_threads: self.game_data.thread_wrapper.capture_snapshot_v1(),
            runtime_state: self.game_data.capture_runtime_state_v1(),
            global_state: GlobalSaveDataV1::capture_hosted(&self.game_data),
        })
    }

    #[cfg(feature = "hosted")]
    pub fn restore_hosted_snapshot(&mut self, snapshot: &HostedCoreSnapshot) -> RfvpResult<()> {
        if snapshot.version != HOSTED_CORE_SNAPSHOT_VERSION
            || self.run_state != RfvpCoreRunState::Booted
        {
            return Err(RfvpError::InvalidData);
        }
        let vm_runner = self.vm_runner.as_mut().ok_or(RfvpError::InvalidData)?;
        snapshot
            .save_state
            .apply(&mut self.game_data, vm_runner.thread_manager_mut())
            .map_err(|_| RfvpError::InvalidData)?;
        if !self.game_data.restore_hosted_globals(&snapshot.globals) {
            return Err(RfvpError::InvalidData);
        }
        self.game_data
            .inputs_manager
            .apply_snapshot_v1(snapshot.input.clone());
        self.game_data
            .timer_manager
            .apply_snapshot_v1(snapshot.timers.clone());
        self.game_data
            .time_mut_ref()
            .apply_snapshot_v1(snapshot.time.clone());
        self.game_data
            .thread_wrapper
            .apply_snapshot_v1(snapshot.deferred_threads.clone());
        self.game_data
            .apply_runtime_state_v1(snapshot.runtime_state.clone());
        snapshot.global_state.apply(&mut self.game_data);
        self.frame_index = snapshot.frame_index;
        self.last_tick_us = snapshot.last_tick_us;
        self.last_dissolve_transitioning = snapshot.last_dissolve_transitioning;
        self.last_dissolve2_transitioning = snapshot.last_dissolve2_transitioning;
        self.quit_requested = snapshot.quit_requested;
        self.last_error = None;
        self.last_error_detail = None;
        Ok(())
    }

    /// Produces a deterministic, host-neutral state representation for
    /// replay and restore verification. Persistence must use
    /// [`Self::capture_hosted_snapshot`] instead.
    #[cfg(feature = "hosted")]
    pub fn canonical_hosted_state_bytes(&self) -> RfvpResult<Vec<u8>> {
        let state = self.capture_canonical_hosted_state()?;
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(&state)
            .map_err(|_| RfvpError::InvalidData)
    }

    #[cfg(feature = "hosted")]
    pub fn canonical_hosted_state_component_hashes(
        &self,
    ) -> RfvpResult<HostedStateComponentHashesV1> {
        let state = self.capture_canonical_hosted_state()?;
        Ok(HostedStateComponentHashesV1 {
            session: hosted_component_hash(&(
                state.version,
                state.frame_index,
                state.last_tick_us,
                state.last_dissolve_transitioning,
                state.last_dissolve2_transitioning,
                state.quit_requested,
            ))?,
            globals: hosted_component_hash(&state.globals)?,
            input_and_time: hosted_component_hash(&(
                &state.input,
                &state.timers,
                &state.time,
                &state.deferred_threads,
            ))?,
            runtime: hosted_component_hash(&(&state.runtime_state, &state.global_state))?,
            motion: hosted_component_hash(&state.motion)?,
            audio: hosted_component_hash(&state.audio)?,
            vm: hosted_component_hash(&state.vm)?,
        })
    }

    #[cfg(feature = "hosted")]
    fn capture_canonical_hosted_state(&self) -> RfvpResult<HostedCanonicalStateV1> {
        if self.run_state != RfvpCoreRunState::Booted {
            return Err(RfvpError::InvalidData);
        }
        let vm_runner = self.vm_runner.as_ref().ok_or(RfvpError::InvalidData)?;
        Ok(HostedCanonicalStateV1 {
            version: 1,
            frame_index: self.frame_index,
            last_tick_us: self.last_tick_us,
            last_dissolve_transitioning: self.last_dissolve_transitioning,
            last_dissolve2_transitioning: self.last_dissolve2_transitioning,
            quit_requested: self.quit_requested,
            globals: self.game_data.capture_hosted_globals(),
            input: self.game_data.inputs_manager.capture_snapshot_v1(),
            timers: self.game_data.timer_manager.capture_snapshot_v1(),
            time: self.game_data.time_ref().capture_snapshot_v1(),
            deferred_threads: self.game_data.thread_wrapper.capture_snapshot_v1(),
            runtime_state: self.game_data.capture_runtime_state_v1(),
            global_state: GlobalSaveDataV1::capture_hosted(&self.game_data),
            motion: self.game_data.motion_manager.capture_canonical_state_v1(),
            audio: AudioSnapshotV1 {
                bgm: self.game_data.bgm_player_ref().capture_snapshot_v1(),
                se: self.game_data.se_player_ref().capture_snapshot_v1(),
            },
            vm: vm_runner.thread_manager().capture_snapshot_v1(),
        })
    }

    pub fn push_event(&mut self, event: RfvpEvent) -> RfvpResult<()> {
        if self.pending_events.len() >= self.config.max_pending_events {
            return Err(RfvpError::CapacityExceeded);
        }
        self.pending_events.push(event);
        Ok(())
    }

    /// Reads one logical RFVP resource from the session-owned pack index.
    ///
    /// Pack bytes are still obtained through the embedding's `RfvpHost` file
    /// port that was used at boot; this method only resolves the bounded
    /// archive entry metadata retained by the core. It never opens an ambient
    /// filesystem path.
    #[cfg(feature = "hosted")]
    pub fn read_hosted_resource(
        &self,
        resource_uri: &str,
        max_bytes: usize,
    ) -> RfvpResult<Vec<u8>> {
        if resource_uri.is_empty() || resource_uri.contains('\0') || max_bytes == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        let (mut stream, known_len) = self
            .game_data
            .vfs
            .open_stream_with_len(resource_uri)
            .map_err(|_| RfvpError::NotFound)?;
        if known_len.is_some_and(|len| len > max_bytes as u64) {
            return Err(RfvpError::CapacityExceeded);
        }

        let read_limit = max_bytes
            .checked_add(1)
            .ok_or(RfvpError::CapacityExceeded)? as u64;
        let capacity = known_len
            .and_then(|len| usize::try_from(len).ok())
            .unwrap_or_default();
        let mut bytes = Vec::with_capacity(capacity);
        stream
            .by_ref()
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| RfvpError::Io)?;
        if bytes.len() > max_bytes {
            return Err(RfvpError::CapacityExceeded);
        }
        Ok(bytes)
    }

    pub fn clear_events(&mut self) {
        self.pending_events.clear();
    }

    /// Drops only the transient renderer upload generations retained by the
    /// core. A hosted embedding owns the actual backend texture lifetime, so
    /// boot and restore boundaries must force the next frame to republish each
    /// live graph instead of trusting a cache from a previous host epoch.
    #[cfg(feature = "hosted")]
    pub fn invalidate_host_render_cache(&mut self) {
        self.render_cache = HostPrimRenderCache::new();
    }

    pub fn boot<H: RfvpHost>(&mut self, host: &mut H, boot: RfvpBootConfig<'_>) -> RfvpResult<()>
    where
        <<H as RfvpHost>::FileSystem as RfvpFileSystem>::File: 'static,
    {
        match self.try_boot(host, boot) {
            Ok(()) => {
                self.run_state = RfvpCoreRunState::Booted;
                self.last_error = None;
                self.last_error_detail = None;
                Ok(())
            }
            Err(err) => {
                self.run_state = RfvpCoreRunState::BootFailed;
                self.last_error = Some(err);
                Err(err)
            }
        }
    }

    fn try_boot<H: RfvpHost>(&mut self, host: &mut H, boot: RfvpBootConfig<'_>) -> RfvpResult<()>
    where
        <<H as RfvpHost>::FileSystem as RfvpFileSystem>::File: 'static,
    {
        if boot.asset_root.as_bytes().iter().any(|b| *b == 0) || boot.asset_root.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        if boot.hcb_extension.as_bytes().iter().any(|b| *b == 0) || boot.hcb_extension.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        if boot.max_hcb_bytes == 0 || boot.max_manifest_entries == 0 {
            return Err(RfvpError::InvalidArgument);
        }

        host.log(
            RfvpLogLevel::Info,
            "rfvp no_std boot: scanning asset root for HCB",
        );

        #[cfg(feature = "old_school")]
        let (hcb, hcb_manifest, parser) = {
            let (path, info, manifest) = find_hcb_info(
                host,
                boot.asset_root,
                boot.hcb_extension,
                boot.max_manifest_entries,
            )?;
            let mut file = host.fs().open(&path)?;
            let len = file.len()?;
            if len == 0 || len as usize > boot.max_hcb_bytes {
                return Err(RfvpError::CapacityExceeded);
            }
            let parser =
                Parser::from_paged_file(file, len as usize, boot.nls, 4096, 8).map_err(|err| {
                    self.last_error_detail = Some(err.to_string());
                    RfvpError::InvalidData
                })?;
            (
                LoadedHcb {
                    path,
                    bytes: Vec::new(),
                    info,
                },
                manifest,
                parser,
            )
        };

        #[cfg(not(feature = "old_school"))]
        let (hcb, hcb_manifest, parser) = {
            let (hcb, hcb_manifest) = find_and_load_hcb(
                host,
                boot.asset_root,
                boot.hcb_extension,
                boot.max_hcb_bytes,
                boot.max_manifest_entries,
            )?;
            let parser = Parser::from_bytes(hcb.bytes.clone(), boot.nls).map_err(|err| {
                self.last_error_detail = Some(err.to_string());
                RfvpError::InvalidData
            })?;
            (hcb, hcb_manifest, parser)
        };
        #[cfg(feature = "old_school")]
        let old_school_scale = load_required_old_school_scale(host).map_err(|(err, detail)| {
            self.last_error_detail = Some(detail);
            err
        })?;
        #[cfg(feature = "old_school")]
        let default_font = load_required_default_font(host).map_err(|(err, detail)| {
            self.last_error_detail = Some(detail);
            err
        })?;
        let mut screen = parser.get_screen_size();
        #[cfg(feature = "old_school")]
        {
            screen = (
                scale_old_school_u32(screen.0, old_school_scale),
                scale_old_school_u32(screen.1, old_school_scale),
            );
        }
        let mut vfs = build_host_vfs(host, boot)?;
        #[cfg(not(feature = "old_school"))]
        vfs.add_loose_file(&hcb.path, hcb.bytes.clone());

        let mut game_data = GameData::default();
        #[cfg(feature = "hosted")]
        game_data.init_hosted_globals(
            parser.get_non_volatile_global_count(),
            parser.get_volatile_global_count(),
        );
        #[cfg(not(feature = "hosted"))]
        GLOBAL.lock().map_err(|_| RfvpError::Backend)?.init_with(
            parser.get_non_volatile_global_count(),
            parser.get_volatile_global_count(),
        );
        #[cfg(feature = "old_school")]
        game_data.set_old_school_scale(old_school_scale);
        #[cfg(feature = "old_school")]
        {
            game_data.fontface_manager = FontEnumerator::from_default_font(default_font);
        }
        #[cfg(not(feature = "old_school"))]
        {
            game_data.fontface_manager = FontEnumerator::new();
        }
        game_data.vfs = vfs;
        game_data.nls = boot.nls;
        game_data.set_window(Window::new(screen, 1.0));

        let mut vm_runner =
            VmRunner::new(crate::subsystem::resources::thread_manager::ThreadManager::new());
        #[cfg(feature = "hosted")]
        vm_runner
            .set_hosted_trace_capacity(self.hosted_trace_capacity)
            .map_err(|_| RfvpError::CapacityExceeded)?;
        vm_runner.start_main(parser.get_entry_point());
        host.log(
            RfvpLogLevel::Info,
            "rfvp no_std boot: parsed real HCB, initialized real GameData, and started VmRunner",
        );

        self.loaded_game = Some(RfvpLoadedGame {
            asset_root: boot.asset_root.to_string(),
            hcb_path: hcb.path.clone(),
            hcb_info: hcb.info,
            hcb_bytes: hcb.bytes,
            hcb_manifest: hcb_manifest
                .into_iter()
                .map(|(path, info)| RfvpResourceEntry { path, info })
                .collect(),
        });
        self.config.virtual_width = screen.0;
        self.config.virtual_height = screen.1;
        self.game_data = game_data;
        self.vm_runner = Some(vm_runner);
        self.parser = Some(parser);
        // Bind the first hosted frame to elapsed host time since boot. Leaving
        // this unset makes the first tick consume a zero delta and permanently
        // shifts script timers and motion state by one presentation.
        self.last_tick_us = Some(host.clock().ticks_us());
        Ok(())
    }

    pub fn tick<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<RfvpTickResult> {
        let now = host.clock().ticks_us();
        let elapsed_us = match self.last_tick_us.replace(now) {
            Some(prev) => now.saturating_sub(prev),
            None => 0,
        };

        let consumed_events = self.pending_events.len();
        self.apply_pending_events_to_game_data();
        let mut quit_requested = false;
        for event in self.pending_events.drain(..) {
            if matches!(event, RfvpEvent::Quit) {
                quit_requested = true;
            }
        }
        if quit_requested {
            self.quit_requested = true;
            host.log(RfvpLogLevel::Info, "quit requested by host event");
        }

        crate::platform_time::set_host_time_us(now);
        self.game_data
            .time_mut_ref()
            .set_external_delta(crate::platform_time::Duration::from_micros(elapsed_us));
        let frame_duration = self.game_data.time_mut_ref().frame();
        let frame_us = frame_duration.as_micros().min(u128::from(u64::MAX)) as u64;
        let frame_time_ms = if frame_us == 0 {
            0
        } else {
            (frame_us + 999) / 1_000
        };
        self.game_data
            .timer_manager
            .tick(frame_time_ms.min(u64::from(u32::MAX)) as u32);
        self.game_data.inputs_manager.begin_frame();
        let video_tick_error = {
            let (video_manager, motion_manager) = (
                &mut self.game_data.video_manager,
                &mut self.game_data.motion_manager,
            );
            video_manager.tick(motion_manager).err()
        };
        if let Some(error) = video_tick_error {
            let message = error.to_string();
            host.log(RfvpLogLevel::Error, &message);
            self.last_error = Some(RfvpError::Unsupported);
            self.last_error_detail = Some(message);
            return Err(RfvpError::Unsupported);
        }
        self.game_data.set_current_thread(0);
        if self.game_data.get_halt() {
            self.game_data.set_halt(false);
        }
        host.audio().tick(elapsed_us)?;
        #[cfg(feature = "hosted")]
        self.sync_hosted_audio_state(host)?;
        #[cfg(feature = "hosted")]
        let loaded_hosted_save = self.process_hosted_load(host)?;
        #[cfg(not(feature = "hosted"))]
        let loaded_hosted_save = false;
        if let (Some(parser), Some(vm_runner)) = (self.parser.as_mut(), self.vm_runner.as_mut()) {
            let dissolve_type = self.game_data.motion_manager.get_dissolve_type();
            let dissolve_transitioning = !matches!(
                dissolve_type,
                crate::subsystem::resources::motion_manager::DissolveType::None
                    | crate::subsystem::resources::motion_manager::DissolveType::Static
            );
            let dissolve2_transitioning =
                self.game_data.motion_manager.is_dissolve2_transitioning();
            let dissolve_completed = (self.last_dissolve_transitioning && !dissolve_transitioning)
                || (self.last_dissolve2_transitioning && !dissolve2_transitioning);
            self.last_dissolve_transitioning = dissolve_transitioning;
            self.last_dissolve2_transitioning = dissolve2_transitioning;
            if dissolve_completed && !loaded_hosted_save {
                if let Err(err) = vm_runner.tick(&mut self.game_data, parser, 0) {
                    let message = err.to_string();
                    host.log(RfvpLogLevel::Error, &message);
                    self.last_error = Some(RfvpError::Unsupported);
                    self.last_error_detail = Some(message);
                    return Err(RfvpError::Unsupported);
                }
            }
            if !loaded_hosted_save {
                if let Err(err) = vm_runner.tick(&mut self.game_data, parser, frame_time_ms) {
                    let message = err.to_string();
                    host.log(RfvpLogLevel::Error, &message);
                    self.last_error = Some(RfvpError::Unsupported);
                    self.last_error_detail = Some(message);
                    return Err(RfvpError::Unsupported);
                }
            }

            // The original engine advances scripts before text and motion updates.
            let mut scene = AnzuScene::new();
            scene.update_after_vm(&mut self.game_data, frame_time_ms);
            self.game_data.set_current_thread(0);

            self.flush_audio(host)?;
            #[cfg(feature = "hosted")]
            self.persist_hosted_file_operations(host)?;
            #[cfg(feature = "hosted")]
            self.persist_hosted_save(host)?;
            self.render_game_frame(host)?;
            self.game_data.inputs_manager.frame_reset();
        } else if self.run_state == RfvpCoreRunState::BootFailed {
            return Err(self.last_error.unwrap_or(RfvpError::InvalidData));
        }
        self.frame_index = self.frame_index.wrapping_add(1);

        Ok(RfvpTickResult {
            frame_index: self.frame_index,
            consumed_events,
            elapsed_us,
        })
    }

    /// Synchronize the script-visible BGM/SE state with the host mixer before
    /// the VM runs.  The host owns decoding and playback, so the player flags
    /// set by `play` cannot remain true after a non-looping stream reaches EOF.
    /// A backend without this query fails the hosted tick at the boundary.
    #[cfg(feature = "hosted")]
    fn sync_hosted_audio_state<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let mut bgm = [false; crate::host_api::BGM_LOGICAL_SLOT_COUNT];
        let mut se = [false; crate::host_api::SE_LOGICAL_SLOT_COUNT];
        for (slot, state) in bgm.iter_mut().enumerate() {
            *state = host.audio().is_playing(AudioStreamId::bgm(slot))?;
        }
        for (slot, state) in se.iter_mut().enumerate() {
            *state = host.audio().is_playing(AudioStreamId::se(slot))?;
        }
        for (slot, state) in bgm.into_iter().enumerate() {
            self.game_data
                .bgm_player_mut()
                .set_playing_from_host(slot, state);
        }
        for (slot, state) in se.into_iter().enumerate() {
            self.game_data
                .se_player_mut()
                .set_playing_from_host(slot, state);
        }
        Ok(())
    }

    #[cfg(feature = "hosted")]
    fn process_hosted_load<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<bool> {
        let Some(slot) = self.game_data.save_manager.take_load_request() else {
            return Ok(false);
        };
        let path = SaveItem::get_save_path(slot);
        let path = path.to_str().ok_or(RfvpError::InvalidData)?;
        let mut file = host.fs().open(path)?;
        let bytes = file.read_to_vec(64 * 1024 * 1024)?;
        let nls = self.game_data.get_nls();
        self.game_data
            .save_manager
            .load_slot_into_current_from_bytes(slot, nls, &bytes)
            .map_err(|_| RfvpError::InvalidData)?;
        let snapshot = try_decode_state_chunk_v1(&bytes)
            .map_err(|_| RfvpError::InvalidData)?
            .ok_or(RfvpError::InvalidData)?;
        let vm_runner = self.vm_runner.as_mut().ok_or(RfvpError::InvalidData)?;
        snapshot
            .apply(&mut self.game_data, vm_runner.thread_manager_mut())
            .map_err(|_| RfvpError::InvalidData)?;
        Ok(true)
    }

    #[cfg(feature = "hosted")]
    fn persist_hosted_save<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let Some((slot, thumb_width, thumb_height)) =
            self.game_data.save_manager.pending_save_capture()
        else {
            return Ok(());
        };
        if slot >= 1000 {
            return Err(RfvpError::InvalidArgument);
        }
        let vm_runner = self.vm_runner.as_ref().ok_or(RfvpError::InvalidData)?;
        let snapshot =
            SaveStateSnapshotV1::capture_hosted(&self.game_data, vm_runner.thread_manager());
        let thumb = render_hosted_save_thumbnail(
            &self.game_data.motion_manager,
            self.config.virtual_width,
            self.config.virtual_height,
            thumb_width.max(1),
            thumb_height.max(1),
        )?;
        let nls = self.game_data.get_nls();
        let bytes = self
            .game_data
            .save_manager
            .build_and_store_hosted_save(slot, thumb, &snapshot, nls)
            .map_err(|_| RfvpError::InvalidData)?;
        let path = SaveItem::get_save_path(slot);
        let path = path.to_str().ok_or(RfvpError::InvalidData)?;
        host.fs().write_all(path, &bytes)?;
        self.game_data.save_manager.consume_save_write_result();
        Ok(())
    }

    #[cfg(feature = "hosted")]
    fn persist_hosted_file_operations<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let operations = self.game_data.save_manager.take_file_operations();
        for operation in operations {
            match operation {
                HostedSaveFileOperation::Refresh => self.refresh_hosted_saves(host)?,
                HostedSaveFileOperation::Remove { slot } => {
                    let path = SaveItem::get_save_path(slot);
                    host.fs()
                        .remove(path.to_str().ok_or(RfvpError::InvalidData)?)?;
                }
                HostedSaveFileOperation::Copy {
                    source,
                    destination,
                } => {
                    let source = SaveItem::get_save_path(source);
                    let destination = SaveItem::get_save_path(destination);
                    host.fs().copy(
                        source.to_str().ok_or(RfvpError::InvalidData)?,
                        destination.to_str().ok_or(RfvpError::InvalidData)?,
                    )?;
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "hosted")]
    fn refresh_hosted_saves<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let mut entries = Vec::new();
        host.fs().list("save", &mut |path, info| {
            if info.kind == crate::host_api::RfvpFileKind::File {
                entries.push(path.to_owned());
            }
            Ok(())
        })?;
        entries.sort();
        self.game_data.save_manager.clear_cached_slots();
        let nls = self.game_data.get_nls();
        for path in entries {
            let Some(slot) = hosted_save_slot_from_path(&path) else {
                continue;
            };
            let mut file = host.fs().open(&path)?;
            let bytes = file.read_to_vec(64 * 1024 * 1024)?;
            self.game_data
                .save_manager
                .load_slot_into_current_from_bytes(slot, nls.clone(), &bytes)
                .map_err(|_| RfvpError::InvalidData)?;
        }
        Ok(())
    }

    pub fn render_empty_frame<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        self.render_game_frame(host)
    }

    pub fn render_status_frame<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        self.render_game_frame(host)
    }

    /// Applies a completed hosted text Hook result before the presentation surface is acquired.
    #[cfg(feature = "hosted")]
    pub fn replace_hosted_text(&mut self, slot: u8, text: &str) -> RfvpResult<()> {
        if slot >= 32 || text.len() >= 512 {
            return Err(RfvpError::InvalidArgument);
        }
        let slot = i32::from(slot);
        self.game_data
            .motion_manager
            .text_manager
            .set_text_content(slot, text);
        self.game_data
            .motion_manager
            .text_upload_slot(slot, &self.game_data.fontface_manager, true)
            .map_err(|_| RfvpError::InvalidData)?;
        Ok(())
    }

    /// Rasterizes the authoritative RFVP scene directly into the currently bound buffer.
    #[cfg(feature = "hosted")]
    pub fn render_hosted_software(&self, renderer: &mut SoftRenderer) -> RfvpResult<()> {
        renderer
            .render_motion(&self.game_data.motion_manager)
            .map_err(|_| RfvpError::InvalidData)
    }

    fn render_game_frame<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let frame = render_motion_to_host(
            host.renderer(),
            &mut self.render_cache,
            &self.game_data.motion_manager,
            (self.config.virtual_width, self.config.virtual_height),
        )?;
        self.hit_proxies = frame.hit_proxies;
        Ok(())
    }

    fn apply_pending_events_to_game_data(&mut self) {
        for event in &self.pending_events {
            match *event {
                RfvpEvent::KeyDown { key, repeat, .. } => {
                    if let Some(keycode) = hosted_keycode(key) {
                        self.game_data
                            .inputs_manager
                            .notify_keycode_down(keycode, repeat);
                    }
                }
                RfvpEvent::KeyUp { key, .. } => {
                    if let Some(keycode) = hosted_keycode(key) {
                        self.game_data.inputs_manager.notify_keycode_up(keycode);
                    }
                }
                RfvpEvent::PointerMove { x, y, in_screen } => {
                    self.game_data.inputs_manager.notify_mouse_move(x, y);
                    self.game_data.inputs_manager.set_mouse_in(in_screen);
                }
                RfvpEvent::PointerDown {
                    button: crate::host_api::PointerButton::Left,
                    ..
                } => {
                    self.game_data.inputs_manager.notify_mouse_down(
                        crate::subsystem::resources::input_manager::KeyCode::MouseLeft,
                    );
                }
                RfvpEvent::PointerUp {
                    button: crate::host_api::PointerButton::Left,
                    ..
                } => {
                    self.game_data.inputs_manager.notify_mouse_up(
                        crate::subsystem::resources::input_manager::KeyCode::MouseLeft,
                    );
                }
                RfvpEvent::Wheel { delta_y, .. } => {
                    self.game_data.inputs_manager.notify_mouse_wheel(delta_y);
                }
                _ => {}
            }
        }
    }

    fn flush_audio<H: RfvpHost>(&mut self, host: &mut H) -> RfvpResult<()> {
        let mut commands = Vec::new();
        self.game_data.audio_manager().drain_commands(&mut commands);
        for command in commands {
            match command {
                crate::rfvp_audio::AudioCommand::LoadEncoded {
                    id,
                    kind,
                    resource_uri,
                    bytes,
                } => {
                    if let Some(resource_uri) = resource_uri {
                        host.audio().load_resource(id, kind, &resource_uri)?;
                    } else {
                        host.audio().load_encoded_owned(id, kind, bytes)?;
                    }
                }
                crate::rfvp_audio::AudioCommand::CreateStream { id, desc } => {
                    host.audio().create_stream(id, desc)?;
                }
                crate::rfvp_audio::AudioCommand::SubmitI16 { id, samples } => {
                    host.audio().submit_i16_owned(id, samples)?;
                }
                crate::rfvp_audio::AudioCommand::SubmitF32 { id, samples } => {
                    host.audio().submit_f32_owned(id, samples)?;
                }
                crate::rfvp_audio::AudioCommand::Play {
                    id,
                    params,
                    fade_in_ms,
                } => {
                    host.audio().play(id, params, fade_in_ms)?;
                }
                crate::rfvp_audio::AudioCommand::Stop { id, fade_ms } => {
                    host.audio().stop(id, fade_ms)?;
                }
                crate::rfvp_audio::AudioCommand::Pause { id } => {
                    host.audio().pause(id)?;
                }
                crate::rfvp_audio::AudioCommand::Resume { id } => {
                    host.audio().resume(id)?;
                }
                crate::rfvp_audio::AudioCommand::SetParams { id, params } => {
                    host.audio().set_params(id, params)?;
                }
                crate::rfvp_audio::AudioCommand::DestroyStream { id } => {
                    host.audio().destroy_stream(id);
                }
                crate::rfvp_audio::AudioCommand::MasterVolume { volume } => {
                    host.audio().set_master_volume(volume)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "hosted")]
fn render_hosted_save_thumbnail(
    motion: &crate::subsystem::resources::motion_manager::MotionManager,
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> RfvpResult<Vec<u8>> {
    let mut renderer = SoftRenderer::new(
        source_width,
        source_height,
        crate::soft_render::PixelFormat::Rgba8,
    )
    .map_err(|_| RfvpError::CapacityExceeded)?;
    renderer
        .render_motion(motion)
        .map_err(|_| RfvpError::InvalidData)?;
    let source = renderer.framebuffer().pixels();
    let target_len = usize::try_from(
        u64::from(target_width)
            .checked_mul(u64::from(target_height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(RfvpError::CapacityExceeded)?,
    )
    .map_err(|_| RfvpError::CapacityExceeded)?;
    let mut target = vec![0; target_len];
    for y in 0..target_height {
        let source_y = u64::from(y) * u64::from(source_height) / u64::from(target_height);
        for x in 0..target_width {
            let source_x = u64::from(x) * u64::from(source_width) / u64::from(target_width);
            let source_offset =
                usize::try_from((source_y * u64::from(source_width) + source_x) * 4)
                    .map_err(|_| RfvpError::CapacityExceeded)?;
            let target_offset =
                usize::try_from((u64::from(y) * u64::from(target_width) + u64::from(x)) * 4)
                    .map_err(|_| RfvpError::CapacityExceeded)?;
            target[target_offset..target_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
    Ok(target)
}

#[cfg(feature = "hosted")]
fn hosted_save_slot_from_path(path: &str) -> Option<u32> {
    let name = path.strip_prefix("save/save")?.strip_suffix(".dat")?;
    if name.len() != 3 || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let slot = name.parse().ok()?;
    (slot < 1000).then_some(slot)
}

#[cfg(feature = "old_school")]
fn load_required_default_font<H: RfvpHost>(host: &mut H) -> Result<Font, (RfvpError, String)> {
    let callbacks = host.platform_callbacks();
    let mut bytes = Vec::new();
    if host
        .fs()
        .read_required_file("defualt.tmap", &mut bytes)
        .is_err()
    {
        notify_fatal(
            callbacks,
            FatalErrorCode::MissingDefaultFont,
            MISSING_OLD_SCHOOL_FONT_MESSAGE,
        );
        return Err((
            RfvpError::NotFound,
            MISSING_OLD_SCHOOL_FONT_MESSAGE.to_string(),
        ));
    }
    match Font::from_old_school_tmap(bytes) {
        Ok(font) => Ok(font),
        Err(_) => {
            notify_fatal(
                callbacks,
                FatalErrorCode::InvalidDefaultFont,
                INVALID_OLD_SCHOOL_FONT_MESSAGE,
            );
            Err((
                RfvpError::InvalidData,
                INVALID_OLD_SCHOOL_FONT_MESSAGE.to_string(),
            ))
        }
    }
}

#[cfg(feature = "old_school")]
fn notify_fatal(callbacks: PlatformCallbacks, code: FatalErrorCode, message: &str) {
    if let Some(callback) = callbacks.fatal_error {
        callback(callbacks.user_data, code, message.as_ptr(), message.len());
    }
}

#[cfg(feature = "old_school")]
fn load_required_old_school_scale<H: RfvpHost>(host: &mut H) -> Result<f32, (RfvpError, String)> {
    let mut bytes = Vec::new();
    if host
        .fs()
        .read_required_file("rfvp.toml", &mut bytes)
        .is_err()
    {
        return Err((
            RfvpError::NotFound,
            MISSING_OLD_SCHOOL_CONFIG_MESSAGE.to_string(),
        ));
    }
    parse_old_school_scale(&bytes).map_err(|detail| (RfvpError::InvalidData, detail))
}

#[cfg(feature = "old_school")]
fn parse_old_school_scale(bytes: &[u8]) -> Result<f32, String> {
    let text =
        core::str::from_utf8(bytes).map_err(|_| INVALID_OLD_SCHOOL_CONFIG_MESSAGE.to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("rfvp.toml is missing required top-level scale field".to_string());
    }

    let mut lines = trimmed.lines();
    let line = lines
        .next()
        .ok_or_else(|| "rfvp.toml is missing required top-level scale field".to_string())?
        .trim();
    if lines.any(|line| !line.trim().is_empty()) {
        return Err("rfvp.toml must contain only one top-level field: scale".to_string());
    }

    let Some((key, value)) = line.split_once('=') else {
        return Err("rfvp.toml is missing required top-level scale field".to_string());
    };
    if key.trim() != "scale" {
        return Err("rfvp.toml must contain only the top-level scale field".to_string());
    }

    let value = value.trim();
    if !(value.contains('.') || value.contains('e') || value.contains('E')) {
        return Err("rfvp.toml scale must be a floating-point number".to_string());
    }

    let scale: f32 = value
        .parse()
        .map_err(|_| "rfvp.toml scale must be a floating-point number".to_string())?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err("rfvp.toml scale must be greater than 0".to_string());
    }
    Ok(scale)
}

#[cfg(feature = "old_school")]
fn scale_old_school_u32(value: u32, scale: f32) -> u32 {
    let scaled = round_half_up(value as f32 * scale);
    scaled.max(0) as u32
}

#[cfg(feature = "old_school")]
fn round_half_up(v: f32) -> i32 {
    if v >= 0.0 {
        (v + 0.5).floor() as i32
    } else {
        (v - 0.5).ceil() as i32
    }
}

fn find_and_load_hcb<H: RfvpHost>(
    host: &mut H,
    root: &str,
    extension: &str,
    max_hcb_bytes: usize,
    max_manifest_entries: usize,
) -> RfvpResult<(LoadedHcb, Vec<(String, RfvpFileInfo)>)> {
    let mut found = Vec::new();
    {
        let visitor = &mut |path: &str, info: RfvpFileInfo| -> RfvpResult<()> {
            if found.len() >= max_manifest_entries {
                return Err(RfvpError::CapacityExceeded);
            }
            found.push((path.to_string(), info));
            Ok(())
        };
        host.fs().enumerate_by_extension(root, extension, visitor)?;
    }
    let Some((path, info)) = found.first().cloned() else {
        return Err(RfvpError::NotFound);
    };
    let mut file = host.fs().open(&path)?;
    let bytes = file.read_to_vec(max_hcb_bytes)?;
    let manifest = found.into_iter().skip(1).collect();
    Ok((LoadedHcb { path, bytes, info }, manifest))
}

#[cfg(feature = "old_school")]
fn find_hcb_info<H: RfvpHost>(
    host: &mut H,
    root: &str,
    extension: &str,
    max_manifest_entries: usize,
) -> RfvpResult<(String, RfvpFileInfo, Vec<(String, RfvpFileInfo)>)> {
    let mut found = Vec::new();
    {
        let visitor = &mut |path: &str, info: RfvpFileInfo| -> RfvpResult<()> {
            if found.len() >= max_manifest_entries {
                return Err(RfvpError::CapacityExceeded);
            }
            found.push((path.to_string(), info));
            Ok(())
        };
        host.fs().enumerate_by_extension(root, extension, visitor)?;
    }
    let Some((path, info)) = found.first().cloned() else {
        return Err(RfvpError::NotFound);
    };
    let manifest = found.into_iter().skip(1).collect();
    Ok((path, info, manifest))
}

struct LoadedHcb {
    path: String,
    bytes: Vec<u8>,
    info: RfvpFileInfo,
}

fn build_host_vfs<H: RfvpHost>(host: &mut H, boot: RfvpBootConfig<'_>) -> RfvpResult<Vfs>
where
    <H::FileSystem as RfvpFileSystem>::File: 'static,
{
    let mut vfs = Vfs::new(boot.nls).map_err(|_| RfvpError::InvalidData)?;
    #[cfg(feature = "old_school")]
    {
        let _ = host;
        let _ = boot;
        return Ok(vfs);
    }

    #[cfg(not(feature = "old_school"))]
    {
        let mut packs = Vec::new();
        {
            let visitor = &mut |path: &str, info: RfvpFileInfo| -> RfvpResult<()> {
                if info.kind == crate::host_api::RfvpFileKind::File {
                    packs.push(path.to_string());
                }
                Ok(())
            };
            host.fs()
                .enumerate_by_extension(boot.asset_root, "bin", visitor)?;
        }
        for path in packs {
            let folder = path
                .rsplit('/')
                .next()
                .unwrap_or(path.as_str())
                .strip_suffix(".bin")
                .unwrap_or(path.as_str());
            let file = host.fs().open(&path)?;
            vfs.add_host_pack(folder, Box::new(file)).map_err(|err| {
                host.log(
                    RfvpLogLevel::Warn,
                    &format!("failed to parse host pack metadata {path}: {err}"),
                );
                RfvpError::InvalidData
            })?;
        }
        Ok(vfs)
    }
}
