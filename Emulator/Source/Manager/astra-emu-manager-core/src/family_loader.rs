//! Loading and driving the independent Family ABI.
//!
//! A loaded module keeps one `FamilyModuleBox` and one `RawLibrary` together.
//! The module object, ABI DTO copies, and root reference are dropped before the
//! library guard. Sessions hold the same guard, so removing a provider from a
//! registry cannot unmap code that an active session still calls.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use abi_stable::{
    abi_stability::abi_checking::{check_layout_compatibility_with_globals, CheckingGlobals},
    library::{lib_header_from_raw_library, RawLibrary},
    sabi_trait::TD_Opaque,
    std_types::RString,
    StableAbi,
};
use astra_emu_family_api::{
    AdvanceRequest, AdvanceResponse, AstraFamilyModuleRef, FamilyDescriptor as AbiDescriptor,
    FamilyError, FamilyModuleBox, FamilyOpen, FamilyProvider, FamilyResult, FamilySession,
    FrameConsumer, FrameConsumerRef, FrameView, FrameVisitor, OpenRequest, OpenResponse,
    ProbeReport, ProbeRequest, SessionRequest,
};
use thiserror::Error;

use crate::family::FamilyPluginDescriptor;
use crate::family_registry::manager_descriptor;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FamilyLoadError {
    #[error("ASTRA_EMU_FAMILY_LOAD_PATH")]
    InvalidPath,
    #[error("ASTRA_EMU_FAMILY_LOAD_LIBRARY")]
    Library,
    #[error("ASTRA_EMU_FAMILY_LOAD_ABI")]
    AbiLayout,
    #[error("ASTRA_EMU_FAMILY_LOAD_MODULE")]
    ModuleInitialization,
    #[error("ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR")]
    Descriptor,
    #[error("ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR: {0}")]
    DescriptorError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN")]
    DuplicatePlugin,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROVIDER")]
    Provider,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROVIDER: {0}")]
    ProviderError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_PROBE")]
    Probe,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROBE: {0}")]
    ProbeError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_POLICY")]
    Policy,
    #[error("ASTRA_EMU_FAMILY_LOAD_POLICY: {0}")]
    PolicyError(String),
}

impl FamilyLoadError {
    /// Stable code suitable for a Manager diagnostic or UI status line.
    pub fn diagnostic_code(&self) -> &str {
        match self {
            Self::InvalidPath => "ASTRA_EMU_FAMILY_LOAD_PATH",
            Self::Library => "ASTRA_EMU_FAMILY_LOAD_LIBRARY",
            Self::AbiLayout => "ASTRA_EMU_FAMILY_LOAD_ABI",
            Self::ModuleInitialization => "ASTRA_EMU_FAMILY_LOAD_MODULE",
            Self::Descriptor => "ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR",
            Self::DescriptorError(error) => error.code(),
            Self::DuplicatePlugin => "ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN",
            Self::Provider => "ASTRA_EMU_FAMILY_LOAD_PROVIDER",
            Self::ProviderError(error) => error.code(),
            Self::Probe => "ASTRA_EMU_FAMILY_LOAD_PROBE",
            Self::ProbeError(error) => error.code(),
            Self::Policy => "ASTRA_EMU_FAMILY_LOAD_POLICY",
            Self::PolicyError(message) => message
                .split_once(':')
                .map(|(code, _)| code)
                .filter(|code| code.starts_with("ASTRA_"))
                .unwrap_or("ASTRA_EMU_FAMILY_LOAD_POLICY"),
        }
    }

    /// Bounded human text for the Manager UI. Raw foreign payloads and paths
    /// are normalized before they enter a `FamilyLoadError`.
    pub fn human_message(&self) -> &str {
        match self {
            Self::DescriptorError(error) | Self::ProviderError(error) | Self::ProbeError(error) => {
                error.message.as_str()
            }
            Self::PolicyError(message) => message
                .split_once(':')
                .map(|(_, message)| message.trim())
                .unwrap_or("family policy rejected the operation"),
            Self::InvalidPath => "the selected family module path is invalid",
            Self::Library => "the family module could not be loaded",
            Self::AbiLayout => "the family module ABI does not match this host",
            Self::ModuleInitialization => "the family module could not initialize",
            Self::Descriptor => "the family module descriptor is invalid",
            Self::DuplicatePlugin => "a family module with this plugin ID is already loaded",
            Self::Provider => "the selected family provider is unavailable",
            Self::Probe => "family probing failed",
            Self::Policy => "family policy rejected the operation",
        }
    }
}

/// The only object that owns a raw library handle. Every ABI object created by
/// the module is stored before that handle, so Rust drop order preserves the
/// dynamic code while vtables and allocator callbacks are still needed.
struct LoadedModuleInner {
    module: FamilyModuleBox,
    descriptor: AbiDescriptor,
    _root: AstraFamilyModuleRef,
    _library: RawLibrary,
}

/// One dynamically loaded plugin. The `Rc` is also held by every live session.
pub struct LoadedFamilyPlugin {
    inner: Rc<LoadedModuleInner>,
    manager_descriptor: FamilyPluginDescriptor,
    location: PathBuf,
}

impl LoadedFamilyPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FamilyLoadError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(FamilyLoadError::InvalidPath);
        }

        let library = RawLibrary::load_at(path).map_err(|_| FamilyLoadError::Library)?;
        let header = unsafe { lib_header_from_raw_library(&library) }
            .map_err(|_| FamilyLoadError::AbiLayout)?;
        let actual_layout = header.layout().ok_or(FamilyLoadError::AbiLayout)?;
        let globals = CheckingGlobals::new();
        if check_layout_compatibility_with_globals(
            <AstraFamilyModuleRef as StableAbi>::LAYOUT,
            actual_layout,
            &globals,
        )
        .is_err()
        {
            return Err(FamilyLoadError::AbiLayout);
        }

        // The layout check above is the safety precondition for this call. A
        // failed initialization drops the root/module while `library` lives.
        let root =
            unsafe { header.init_root_module_with_unchecked_layout::<AstraFamilyModuleRef>() }
                .map_err(|_| FamilyLoadError::ModuleInitialization)?;
        let module = root.service().get();
        let descriptor = module
            .descriptor()
            .into_result()
            .map_err(|error| FamilyLoadError::DescriptorError(host_owned_error(error)))?;
        descriptor
            .validate()
            .map_err(|error| FamilyLoadError::DescriptorError(host_owned_error(error)))?;
        let descriptor = host_owned_descriptor(descriptor);
        let manager_descriptor = manager_descriptor(&descriptor)?;

        let inner = Rc::new(LoadedModuleInner {
            module,
            descriptor,
            _root: root,
            _library: library,
        });
        Ok(Self {
            inner,
            manager_descriptor,
            location: path.to_owned(),
        })
    }

    pub fn location(&self) -> &Path {
        &self.location
    }

    pub fn manager_descriptor(&self) -> &FamilyPluginDescriptor {
        &self.manager_descriptor
    }
}

impl FamilyProvider for LoadedFamilyPlugin {
    fn descriptor(&self) -> FamilyResult<AbiDescriptor> {
        Ok(self.inner.descriptor.clone())
    }

    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate()?;
        let result = self
            .inner
            .module
            .probe(request)
            .into_result()
            .map_err(host_owned_error)?;
        let Some(report) = result.into_option() else {
            return Ok(None);
        };
        report.validate().map_err(host_owned_error)?;
        if report.family_id.as_str() != self.manager_descriptor.family_id
            || !self
                .manager_descriptor
                .supported_formats
                .iter()
                .any(|format| format == report.format.as_str())
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_PROBE_DESCRIPTOR",
                "probe report does not match its descriptor",
            ));
        }
        Ok(Some(host_owned_probe_report(report)))
    }

    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        request.validate_for_descriptor(&self.inner.descriptor)?;
        let response = self
            .inner
            .module
            .open(request)
            .into_result()
            .map_err(host_owned_error)?;
        if let Err(error) = response.validate_for_descriptor(&self.inner.descriptor) {
            // `open` may have started workers before returning an invalid
            // response. Give the family one synchronous close opportunity
            // while the module and library are definitely still alive.
            let close_result = self
                .inner
                .module
                .close(SessionRequest {
                    session_id: response.session_id.clone(),
                })
                .into_result()
                .map_err(host_owned_error);
            if let Err(close_error) = close_result {
                tracing::error!(
                    event = "astra.emu.family.open_cleanup_failed",
                    diagnostic = %close_error.code(),
                );
            }
            return Err(host_owned_error(error));
        }
        let response = host_owned_open_response(response);
        let session_id = response.session_id.clone();
        Ok(FamilyOpen {
            response,
            session: Box::new(LoadedFamilySession {
                inner: Rc::clone(&self.inner),
                session_id,
                closed: false,
            }),
        })
    }
}

struct LoadedFamilySession {
    inner: Rc<LoadedModuleInner>,
    session_id: RString,
    closed: bool,
}

impl FamilySession for LoadedFamilySession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[astra_emu_family_api::FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        let request = AdvanceRequest {
            session_id: self.session_id.clone(),
            elapsed_ns,
            events: events.to_vec().into(),
        };
        request.validate()?;
        self.inner
            .module
            .advance(request)
            .into_result()
            .map_err(host_owned_error)
    }

    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        let mut bridge = FrameForwarder {
            visitor,
            frame_count: 0,
            callback_error: None,
        };
        // `FrameConsumerRef` is tied to this stack borrow and can only be used
        // for the synchronous `frame` call. No foreign frame bytes or callback
        // object survive the call, so unloading remains guarded by `inner`.
        let consumer: FrameConsumerRef<'_> = FrameConsumerRef::from_ptr(&mut bridge, TD_Opaque);
        let result = self
            .inner
            .module
            .frame(
                SessionRequest {
                    session_id: self.session_id.clone(),
                },
                consumer,
            )
            .into_result()
            .map_err(host_owned_error);
        if let Some(error) = bridge.callback_error.take() {
            return Err(host_owned_error(error));
        }
        result?;
        Ok(())
    }

    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        self.closed = true;
        self.inner
            .module
            .close(SessionRequest {
                session_id: self.session_id.clone(),
            })
            .into_result()
            .map_err(host_owned_error)
    }
}

impl Drop for LoadedFamilySession {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        // FamilySession::close is the normal lifecycle. A dropped live
        // session still gets a best-effort synchronous stop before its Rc
        // guard is released; the diagnostic makes misuse observable.
        let result = self
            .inner
            .module
            .close(SessionRequest {
                session_id: self.session_id.clone(),
            })
            .into_result();
        if let Err(error) = result {
            tracing::error!(
                event = "astra.emu.family.session_drop_cleanup_failed",
                diagnostic = %error.code(),
            );
        } else {
            tracing::warn!(event = "astra.emu.family.session_dropped_without_close");
        }
    }
}

struct FrameForwarder<'a> {
    visitor: &'a mut dyn FrameVisitor,
    frame_count: usize,
    callback_error: Option<FamilyError>,
}

impl FrameConsumer for FrameForwarder<'_> {
    fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FfiFamilyResult<()> {
        if self.frame_count != 0 {
            return self.error(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_COUNT",
                "family emitted more than one frame",
            ));
        }
        let required = match frame.info.required_bytes() {
            Some(required) => required,
            None => {
                return self.error(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_FRAME_SIZE",
                    "frame size does not fit host usize",
                ))
            }
        };
        if let Err(error) = frame.info.validate() {
            return self.error(error);
        }
        let pixels = frame.as_slice();
        if pixels.len() < required {
            return self.error(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_BYTES",
                "frame pixels are shorter than stride times height",
            ));
        }
        self.frame_count = 1;
        match self.visitor.accept(frame) {
            Ok(()) => astra_emu_family_api::FfiFamilyResult::ROk(()),
            Err(error) => self.error(error),
        }
    }
}

impl FrameForwarder<'_> {
    fn error(&mut self, error: FamilyError) -> astra_emu_family_api::FfiFamilyResult<()> {
        if self.callback_error.is_none() {
            self.callback_error = Some(error.clone());
        }
        astra_emu_family_api::FfiFamilyResult::RErr(error)
    }
}

pub(crate) fn host_owned_error(error: FamilyError) -> FamilyError {
    FamilyError::new(
        error.code().to_owned(),
        host_owned_message(error.message.as_str()),
    )
}

fn host_owned_message(message: &str) -> String {
    const MAX_MESSAGE_CHARS: usize = 512;
    let message = message.lines().next().unwrap_or_default().trim();
    if message.is_empty()
        || message.starts_with('/')
        || message.starts_with('\\')
        || message.contains(":\\")
        || message.contains(":/")
        || message.contains("://")
    {
        return "family operation failed".into();
    }
    message
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_MESSAGE_CHARS)
        .collect()
}

fn host_owned_descriptor(descriptor: AbiDescriptor) -> AbiDescriptor {
    AbiDescriptor {
        family_id: descriptor.family_id.as_str().to_owned().into(),
        plugin_id: descriptor.plugin_id.as_str().to_owned().into(),
        abi_fingerprint: descriptor.abi_fingerprint.as_str().to_owned().into(),
        version: descriptor.version.as_str().to_owned().into(),
        capabilities: descriptor
            .capabilities
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .into(),
        supported_formats: descriptor
            .supported_formats
            .iter()
            .map(|format| RString::from(format.as_str().to_owned()))
            .collect::<Vec<_>>()
            .into(),
    }
}

fn host_owned_probe_report(report: ProbeReport) -> ProbeReport {
    ProbeReport {
        family_id: report.family_id.as_str().to_owned().into(),
        game_id: report.game_id.as_str().to_owned().into(),
        format: report.format.as_str().to_owned().into(),
        confidence_permyriad: report.confidence_permyriad,
    }
}

fn host_owned_open_response(response: OpenResponse) -> OpenResponse {
    OpenResponse {
        session_id: response.session_id.as_str().to_owned().into(),
        frame: response.frame,
        audio_format: response.audio_format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{FamilyResult, FrameAlpha, FrameFormat, FrameInfo};

    struct CountingVisitor(usize);

    impl FrameVisitor for CountingVisitor {
        fn accept(&mut self, _frame: FrameView<'_>) -> FamilyResult<()> {
            self.0 += 1;
            Ok(())
        }
    }

    #[test]
    fn frame_forwarder_latches_errors_when_plugin_ignores_callback_failure() {
        let info = FrameInfo {
            width: 1,
            height: 1,
            stride: 4,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        };
        let mut visitor = CountingVisitor(0);
        let mut bridge = FrameForwarder {
            visitor: &mut visitor,
            frame_count: 0,
            callback_error: None,
        };
        let first = FrameView::from_slice(&[0, 0, 0, 255], info).unwrap();
        assert!(bridge.accept(first).into_result().is_ok());
        let second = FrameView::from_slice(&[0, 0, 0, 255], info).unwrap();
        assert!(bridge.accept(second).into_result().is_err());
        assert_eq!(
            bridge.callback_error.as_ref().map(FamilyError::code),
            Some("ASTRA_EMU_FAMILY_FRAME_COUNT")
        );
        drop(bridge);
        assert_eq!(visitor.0, 1);
    }
}
