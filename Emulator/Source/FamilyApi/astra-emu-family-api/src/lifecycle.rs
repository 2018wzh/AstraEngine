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
    frame::{FrameConsumerRef, FrameInfo, FrameVisitor},
    input::{validate_events, FamilyEvent, WindowState},
    text::TextReplacementServiceBox,
};

#[repr(C)]
#[derive(StableAbi)]
pub struct FamilyHostServices {
    pub audio_sink: ROption<AudioSinkBox>,
    pub text_replacement: ROption<TextReplacementServiceBox>,
}

#[repr(C)]
#[derive(StableAbi)]
pub struct OpenRequest {
    pub game_path: RString,
    pub initial_window: WindowState,
    pub host: FamilyHostServices,
}

impl OpenRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_game_path(&self.game_path)?;
        self.initial_window.validate()
    }

    pub fn validate_for_descriptor(&self, descriptor: &FamilyDescriptor) -> FamilyResult<()> {
        self.validate()?;
        if descriptor.has_capability(crate::FamilyCapability::PcmAudio)
            && !matches!(&self.host.audio_sink, ROption::RSome(_))
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_AUDIO_SINK",
                "family declares PCM audio but the host supplied no audio sink",
            ));
        }
        if descriptor.has_capability(crate::FamilyCapability::TextReplacement)
            && !matches!(&self.host.text_replacement, ROption::RSome(_))
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_TEXT_SERVICE",
                "family declares text replacement but the host supplied no text service",
            ));
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct AdvanceResponse {
    pub status: FamilyStatus,
}

impl AdvanceResponse {
    pub fn running() -> Self {
        Self {
            status: FamilyStatus::Running,
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
    fn descriptor(&self) -> FfiFamilyResult<FamilyDescriptor>;
    fn probe(&self, request: ProbeRequest) -> FfiFamilyResult<ROption<ProbeReport>>;
    fn open(&self, request: OpenRequest) -> FfiFamilyResult<OpenResponse>;
    fn advance(&self, request: AdvanceRequest) -> FfiFamilyResult<AdvanceResponse>;
    fn frame(&self, request: SessionRequest, consumer: FrameConsumerRef<'_>)
        -> FfiFamilyResult<()>;
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
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()>;
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
