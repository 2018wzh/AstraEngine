#![allow(non_local_definitions)]

use abi_stable::{
    library::RootModule,
    sabi_trait,
    sabi_types::{Constructor, VersionStrings},
    std_types::{RBox, ROption, RString, RVec},
    StableAbi,
};

use super::{
    audio::{AudioSinkBox, PcmFormatSpec},
    descriptor::{
        validate_game_path, validate_symbol, FamilyDescriptor, FamilyError, FamilyResult,
        FfiFamilyResult, ProbeReport, ProbeRequest,
    },
    frame::{FrameIdentity, FrameInfo, FramePoll, FrameRequest},
    input::{validate_events, FamilyEvent, WindowState},
    modal::ModalServiceBox,
    text::TextReplacementServiceBox,
};

#[repr(C)]
#[derive(StableAbi)]
pub struct FamilyHostServices {
    pub audio_sink: ROption<AudioSinkBox>,
    pub text_replacement: ROption<TextReplacementServiceBox>,
    pub modal: ROption<ModalServiceBox>,
}

#[repr(C)]
#[derive(StableAbi)]
pub struct OpenRequest {
    pub game_path: RString,
    pub session_epoch: u64,
    pub configuration: RVec<crate::ConfigEntry>,
    pub initial_window: WindowState,
    pub host: FamilyHostServices,
}

impl OpenRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_game_path(&self.game_path)?;
        FrameIdentity {
            session_epoch: self.session_epoch,
            generation: 0,
        }
        .validate(false)?;
        self.initial_window.validate()
    }

    pub fn validate_for_descriptor(&self, descriptor: &FamilyDescriptor) -> FamilyResult<()> {
        descriptor.validate()?;
        self.validate()?;
        if descriptor.has_capability(crate::FamilyCapability::PcmAudio)
            && !matches!(&self.host.audio_sink, ROption::RSome(_))
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_AUDIO_SINK",
                "family declares PCM audio but the host supplied no audio sink",
            ));
        }
        crate::resolve_config(&descriptor.configuration, &self.configuration)?;
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct OpenResponse {
    pub session_id: RString,
    pub frame: FrameInfo,
    pub audio_format: ROption<PcmFormatSpec>,
}

impl OpenResponse {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("session_id", &self.session_id)?;
        self.frame.validate()?;
        if let ROption::RSome(audio) = self.audio_format {
            audio.validate()?;
        }
        Ok(())
    }

    pub fn validate_for_descriptor(&self, descriptor: &FamilyDescriptor) -> FamilyResult<()> {
        self.validate()?;
        let has_audio = matches!(self.audio_format, ROption::RSome(_));
        if descriptor.has_capability(crate::FamilyCapability::PcmAudio) != has_audio {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_AUDIO_DECLARATION",
                "PCM audio capability and open audio format must agree",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct AdvanceRequest {
    pub session_id: RString,
    /// Elapsed wall time since the previous call. Zero is valid for an
    /// explicitly polled frame; the family owns its tick policy.
    pub elapsed_ns: u64,
    pub events: RVec<FamilyEvent>,
}

impl AdvanceRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("session_id", &self.session_id)?;
        validate_events(&self.events)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FamilyStatus {
    Running,
    Waiting,
    Finished,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub enum FamilyWindowCommand {
    /// Request the host-owned game window's fullscreen mode. No native handle
    /// crosses the ABI. Hosts must execute this on their window thread or fail
    /// the session explicitly when window control is unavailable.
    SetFullscreen(bool),
    /// UTF-8 window caption. Empty clears it; native encodings are decoded by the family.
    SetTitle(RString),
    /// Owned, tightly packed straight-alpha RGBA8 small window icon.
    SetIcon(FamilyWindowIcon),
    /// Update the host-owned pointer in logical stage coordinates. Omitted
    /// properties retain their current values; no native cursor handle is passed.
    SetPointer(FamilyPointerRequest),
}

#[repr(C)]
#[derive(Debug, Clone, Default, PartialEq, Eq, StableAbi)]
pub struct FamilyPointerRequest {
    pub logical_x: ROption<i32>,
    pub logical_y: ROption<i32>,
    pub visible: ROption<bool>,
    /// Idle time in milliseconds; zero disables automatic hiding. Actual
    /// pointer activity restores visibility unless explicitly hidden.
    pub autohide_ms: ROption<u64>,
}

impl FamilyPointerRequest {
    /// Coalesce pending updates without discarding unchanged properties.
    pub fn merge(&mut self, next: Self) {
        if next.logical_x.is_some() {
            self.logical_x = next.logical_x;
        }
        if next.logical_y.is_some() {
            self.logical_y = next.logical_y;
        }
        if next.visible.is_some() {
            self.visible = next.visible;
        }
        if next.autohide_ms.is_some() {
            self.autohide_ms = next.autohide_ms;
        }
    }
}

pub const MAX_WINDOW_TITLE_BYTES: usize = 4096;
pub const MAX_WINDOW_ICON_DIMENSION: u32 = 256;

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FamilyWindowIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: RVec<u8>,
}

impl FamilyWindowIcon {
    pub fn validate(&self) -> FamilyResult<()> {
        let expected = self
            .width
            .checked_mul(self.height)
            .and_then(|n| n.checked_mul(4));
        if self.width == 0
            || self.height == 0
            || self.width > MAX_WINDOW_ICON_DIMENSION
            || self.height > MAX_WINDOW_ICON_DIMENSION
            || expected.map(|n| n as usize) != Some(self.rgba.len())
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_WINDOW_ICON",
                "window icon dimensions or RGBA8 byte length are invalid",
            ));
        }
        Ok(())
    }
}

impl FamilyWindowCommand {
    pub fn validate(&self) -> FamilyResult<()> {
        if let Self::SetIcon(icon) = self {
            return icon.validate();
        }
        if let Self::SetTitle(title) = self {
            if title.len() > MAX_WINDOW_TITLE_BYTES || title.chars().any(char::is_control) {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_WINDOW_TITLE",
                    "window title exceeds its UTF-8 bound or contains control characters",
                ));
            }
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct AdvanceResponse {
    pub status: FamilyStatus,
    pub window_command: ROption<FamilyWindowCommand>,
    /// A successful restore, explicit session restart, or Family-defined
    /// pause/resume transition established a new playback timeline. Start the
    /// next wall-time interval after this call and its frame capture complete;
    /// do not replay loading or paused time.
    pub reset_clock: bool,
}

impl AdvanceResponse {
    pub fn validate(&self) -> FamilyResult<()> {
        if let ROption::RSome(command) = &self.window_command {
            command.validate()?;
        }
        Ok(())
    }

    pub fn running() -> Self {
        Self {
            status: FamilyStatus::Running,
            window_command: ROption::RNone,
            reset_clock: false,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct SessionRequest {
    pub session_id: RString,
}

impl SessionRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("session_id", &self.session_id)
    }
}

/// Operations exported by a family module. `abi_stable` generates the
/// cross-library vtable, so no raw function pointer or opaque callback field
/// is needed in the root module.
#[sabi_trait]
pub trait FamilyModule {
    /// Install the process-lifetime host diagnostic sink before descriptor/probe/open.
    fn initialize_diagnostics(&self, sink: crate::DiagnosticSinkBox) -> FfiFamilyResult<()>;
    fn descriptor(&self) -> FfiFamilyResult<FamilyDescriptor>;
    fn probe(&self, request: ProbeRequest) -> FfiFamilyResult<ROption<ProbeReport>>;
    /// Opens a family session.
    ///
    /// An implementation that returns an error must already have cancelled
    /// every host call and stopped and joined every worker it created while
    /// opening. An `Err` result is not permission to leave callbacks or
    /// background threads alive. The library remains resident until process exit.
    fn open(&self, request: OpenRequest) -> FfiFamilyResult<OpenResponse>;
    fn advance(&self, request: AdvanceRequest) -> FfiFamilyResult<AdvanceResponse>;
    fn frame(&self, request: FrameRequest<'_>) -> FfiFamilyResult<FramePoll>;
    /// Closes the named session and releases all family-owned resources.
    ///
    /// Before returning, including when returning `Err`, the implementation
    /// must cancel in-flight host calls and stop and join all family workers.
    /// The library remains resident, but an error cannot
    /// be used to retain a worker or a host callback. If a family owns audio
    /// and video workers, it must complete shutdown for both before returning
    /// the first error; an early `audio.close()` must not skip video cleanup.
    fn close(&self, request: SessionRequest) -> FfiFamilyResult<()>;
}

pub type FamilyModuleBox = FamilyModule_TO<'static, RBox<()>>;
pub type FamilyModuleConstructor = Constructor<FamilyModuleBox>;

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(prefix_ref = AstraFamilyModuleRef, prefix_fields = AstraFamilyModulePrefix)))]
#[sabi(missing_field(panic))]
pub struct AstraFamilyModule {
    #[sabi(last_prefix_field)]
    pub service: FamilyModuleConstructor,
}

impl RootModule for AstraFamilyModuleRef {
    abi_stable::declare_root_module_statics! {AstraFamilyModuleRef}

    const BASE_NAME: &'static str = "astra_emu_independent_family_module";
    const NAME: &'static str = "astra-emu-independent-family";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}

pub struct FamilyOpen {
    pub response: OpenResponse,
    pub session: Box<dyn FamilySession>,
}

pub trait FamilyProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor>;
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>>;
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen>;
}

pub trait FamilySession {
    fn advance(&mut self, elapsed_ns: u64, events: &[FamilyEvent])
        -> FamilyResult<AdvanceResponse>;
    fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll>;
    /// Stops the session and releases all family-owned resources.
    ///
    /// Before returning, including when returning `Err`, the implementation
    /// must cancel in-flight host calls and stop and join every family worker.
    /// The library remains resident, but an error must never leave a worker
    /// or host callback running. When
    /// audio and video workers are present, both shutdown paths must finish
    /// before the first error is returned.
    fn close(self: Box<Self>) -> FamilyResult<()>;
}

pub(crate) fn validate_dimensions(width: u32, height: u32) -> FamilyResult<()> {
    if width == 0 || height == 0 {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_FRAME_SIZE",
            "dimensions must be non-zero",
        ));
    }
    Ok(())
}

pub(crate) fn validate_window_size(width: u32, height: u32) -> FamilyResult<()> {
    if width == 0 || height == 0 {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_WINDOW_SIZE",
            "window dimensions must be non-zero",
        ));
    }
    Ok(())
}
