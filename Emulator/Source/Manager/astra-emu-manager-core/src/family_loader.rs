//! Loading and driving the independent Family ABI.
//!
//! A loaded module keeps one `FamilyModuleBox` and one `RawLibrary` together.
//! The module object, ABI DTO copies, and root reference are dropped before the
//! library guard. Sessions hold the same guard, so removing a provider from a
//! registry cannot unmap code that an active session still calls.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use abi_stable::{
    abi_stability::{check_layout_compatibility_with_globals, CheckingGlobals},
    library::{lib_header_from_raw_library, RawLibrary, RootModule},
    sabi_types::TD_Opaque,
    std_types::RString,
    StableAbi,
};
use astra_emu_family_api::{
    AdvanceRequest, AdvanceResponse, AstraFamilyModuleRef, FamilyCapability as AbiCapability,
    FamilyDescriptor as AbiDescriptor, FamilyError, FamilyModule, FamilyModuleBox,
    FamilyOpen, FamilyProvider, FamilyResult, FamilySession, FrameConsumer, FrameConsumerBox,
    FrameInfo, FrameView, FrameVisitor, OpenRequest, OpenResponse, ProbeReport, ProbeRequest,
    SessionRequest,
};
use thiserror::Error;

use crate::family::{
    FamilyCapability, FamilyPluginDescriptor, FamilyPluginRegistry, FamilyProbeReport,
    FamilyProbeSelection,
};

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
    #[error("ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN")]
    DuplicatePlugin,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROVIDER")]
    Provider,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROBE")]
    Probe,
    #[error("ASTRA_EMU_FAMILY_LOAD_POLICY")]
    Policy,
}

/// The only object that owns a raw library handle. Every ABI object created by
/// the module is stored before that handle, so Rust drop order preserves the
/// dynamic code while vtables and allocator callbacks are still needed.
struct LoadedModuleInner {
    module: FamilyModuleBox,
    descriptor: AbiDescriptor,
    root: AstraFamilyModuleRef,
    library: RawLibrary,
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
        let root = unsafe {
            header.init_root_module_with_unchecked_layout::<AstraFamilyModuleRef>()
        }
        .map_err(|_| FamilyLoadError::ModuleInitialization)?;
        let module = root.service().get();
        let descriptor = module
            .descriptor()
            .into_result()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        descriptor
            .validate()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        let descriptor = host_owned_descriptor(descriptor);
        let manager_descriptor = manager_descriptor(&descriptor)?;

        let inner = Rc::new(LoadedModuleInner {
            module,
            descriptor,
            root,
            library,
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
        let Some(report) = result else {
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
        let frames = Rc::new(std::cell::RefCell::new(Vec::new()));
        let callback_error = Rc::new(std::cell::RefCell::new(None));
        let bridge = FrameForwarder {
            frames: Rc::clone(&frames),
            callback_error: Rc::clone(&callback_error),
        };
        // The ABI callback is erased as owned data, but it never carries the
        // caller's borrowed visitor. It copies each synchronous frame into a
        // host-owned buffer; the visitor runs only after the ABI call returns.
        let consumer: FrameConsumerBox = FrameConsumerBox::from_value(bridge, TD_Opaque);
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
        if let Err(error) = result {
            return Err(error);
        }
        if let Some(error) = callback_error.borrow_mut().take() {
            return Err(error);
        }
        let collected = frames.borrow_mut().drain(..).collect::<Vec<_>>();
        for frame in collected {
            let view = FrameView::from_slice(&frame.pixels, frame.info)?;
            visitor.accept(view)?;
        }
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

struct CollectedFrame {
    info: FrameInfo,
    pixels: Vec<u8>,
}

struct FrameForwarder {
    frames: Rc<std::cell::RefCell<Vec<CollectedFrame>>>,
    callback_error: Rc<std::cell::RefCell<Option<FamilyError>>>,
}

impl FrameConsumer for FrameForwarder {
    fn accept(&mut self, frame: FrameView<'_>) -> astra_emu_family_api::FfiFamilyResult<()> {
        if !self.frames.borrow().is_empty() {
            let error = FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_COUNT",
                "family emitted more than one frame",
            );
            *self.callback_error.borrow_mut() = Some(error.clone());
            return astra_emu_family_api::FfiFamilyResult::RErr(error);
        }
        let required = match frame.info.required_bytes() {
            Some(required) => required,
            None => {
                let error = FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_FRAME_SIZE",
                    "frame size does not fit host usize",
                );
                *self.callback_error.borrow_mut() = Some(error.clone());
                return astra_emu_family_api::FfiFamilyResult::RErr(error);
            }
        };
        if let Err(error) = frame.info.validate() {
            *self.callback_error.borrow_mut() = Some(error.clone());
            return astra_emu_family_api::FfiFamilyResult::RErr(error);
        }
        let pixels = frame.as_slice();
        if pixels.len() < required {
            let error = FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_BYTES",
                "frame pixels are shorter than stride times height",
            );
            *self.callback_error.borrow_mut() = Some(error.clone());
            return astra_emu_family_api::FfiFamilyResult::RErr(error);
        }
        let mut owned = Vec::new();
        if owned.try_reserve_exact(required).is_err() {
            let error = FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_ALLOC",
                "host could not reserve frame storage",
            );
            *self.callback_error.borrow_mut() = Some(error.clone());
            return astra_emu_family_api::FfiFamilyResult::RErr(error);
        }
        owned.extend_from_slice(&pixels[..required]);
        self.frames.borrow_mut().push(CollectedFrame {
            info: frame.info,
            pixels: owned,
        });
        astra_emu_family_api::FfiFamilyResult::ROk(())
    }
}

/// Runtime registry used by Manager and CLI. Static and dynamic providers are
/// registered through the same descriptor validation and probe-selection path.
pub struct FamilyProviderRegistry {
    providers: BTreeMap<String, Box<dyn FamilyProvider>>,
    descriptors: FamilyPluginRegistry,
}

impl Default for FamilyProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FamilyProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: BTreeMap::new(),
            descriptors: FamilyPluginRegistry::new(),
        }
    }

    pub fn register_provider<P>(&mut self, provider: P) -> Result<(), FamilyLoadError>
    where
        P: FamilyProvider + 'static,
    {
        let descriptor = provider
            .descriptor()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        descriptor
            .validate()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        let manager = manager_descriptor(&descriptor)?;
        let plugin_id = manager.plugin_id.clone();
        if self.providers.contains_key(&plugin_id) {
            return Err(FamilyLoadError::DuplicatePlugin);
        }
        self.descriptors
            .register(manager)
            .map_err(|_| FamilyLoadError::DuplicatePlugin)?;
        self.providers.insert(plugin_id, Box::new(provider));
        Ok(())
    }

    pub fn load_dynamic(&mut self, path: impl AsRef<Path>) -> Result<(), FamilyLoadError> {
        self.register_provider(LoadedFamilyPlugin::load(path)?)
    }

    pub fn remove(&mut self, plugin_id: &str) -> bool {
        self.descriptors.remove(plugin_id);
        self.providers.remove(plugin_id).is_some()
    }

    pub fn descriptor(&self, plugin_id: &str) -> Option<&FamilyPluginDescriptor> {
        self.descriptors.descriptor(plugin_id)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &FamilyPluginDescriptor> {
        self.descriptors.descriptors()
    }

    pub fn probe(
        &self,
        request: &ProbeRequest,
        preferred_plugin_id: Option<&str>,
        preferred_family_id: Option<&str>,
    ) -> Result<FamilyProbeSelection, FamilyLoadError> {
        request.validate().map_err(|_| FamilyLoadError::Probe)?;
        let mut reports = Vec::new();
        for (plugin_id, provider) in &self.providers {
            let Some(report) = provider
                .probe(request.clone())
                .map_err(|_| FamilyLoadError::Probe)?
            else {
                continue;
            };
            reports.push(FamilyProbeReport {
                plugin_id: plugin_id.clone(),
                family_id: report.family_id.to_string(),
                game_id: report.game_id.to_string(),
                format: report.format.to_string(),
                confidence_permyriad: report.confidence_permyriad,
            });
        }
        self.descriptors
            .select_probe(reports, preferred_plugin_id, preferred_family_id)
            .map_err(|_| FamilyLoadError::Policy)
    }

    pub fn open_selected(
        &mut self,
        candidate: &crate::family::FamilyProbeCandidate,
        request: OpenRequest,
    ) -> Result<FamilyOpen, FamilyLoadError> {
        let checked = self
            .descriptors
            .choose_probe(
                std::slice::from_ref(candidate),
                &candidate.report.plugin_id,
                &candidate.report.game_id,
            )
            .map_err(|_| FamilyLoadError::Policy)?;
        let provider = self
            .providers
            .get_mut(&checked.report.plugin_id)
            .ok_or(FamilyLoadError::Provider)?;
        provider.open(request).map_err(|_| FamilyLoadError::Provider)
    }
}

fn host_owned_error(error: FamilyError) -> FamilyError {
    FamilyError::new(error.code().to_owned(), error.message.as_str().to_owned())
}

fn host_owned_descriptor(descriptor: AbiDescriptor) -> AbiDescriptor {
    AbiDescriptor {
        family_id: descriptor.family_id.as_str().to_owned().into(),
        plugin_id: descriptor.plugin_id.as_str().to_owned().into(),
        abi_fingerprint: descriptor.abi_fingerprint.as_str().to_owned().into(),
        version: descriptor.version.as_str().to_owned().into(),
        capabilities: descriptor.capabilities.iter().copied().collect::<Vec<_>>().into(),
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

fn manager_descriptor(
    descriptor: &AbiDescriptor,
) -> Result<FamilyPluginDescriptor, FamilyLoadError> {
    let capabilities = descriptor
        .capabilities
        .iter()
        .map(|capability| match capability {
            AbiCapability::CpuFrame => FamilyCapability::CpuFrame,
            AbiCapability::PcmAudio => FamilyCapability::PcmAudio,
            AbiCapability::NativeSave => FamilyCapability::NativeSave,
            AbiCapability::TextReplacement => FamilyCapability::TextReplacement,
        })
        .collect();
    let result = FamilyPluginDescriptor {
        family_id: descriptor.family_id.to_string(),
        plugin_id: descriptor.plugin_id.to_string(),
        abi_fingerprint: descriptor.abi_fingerprint.to_string(),
        version: descriptor.version.to_string(),
        capabilities,
        supported_formats: descriptor
            .supported_formats
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    result.validate().map_err(|_| FamilyLoadError::Descriptor)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{
        FamilyCapability as AbiCapability, FamilyDescriptor, FamilyModuleBox, FamilyResult,
        OpenRequest, ProbeRequest,
    };

    struct ProbeProvider {
        descriptor: FamilyDescriptor,
        report: Option<ProbeReport>,
    }

    impl FamilyProvider for ProbeProvider {
        fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
            Ok(host_owned_descriptor(self.descriptor.clone()))
        }

        fn probe(&self, _request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
            Ok(self.report.clone().map(host_owned_probe_report))
        }

        fn open(&mut self, _request: OpenRequest) -> FamilyResult<FamilyOpen> {
            unreachable!("probe registry test does not open sessions")
        }
    }

    fn descriptor(plugin_id: &str, family_id: &str) -> FamilyDescriptor {
        FamilyDescriptor {
            family_id: family_id.into(),
            plugin_id: plugin_id.into(),
            abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
            version: "1.0.0".into(),
            capabilities: vec![AbiCapability::CpuFrame].into(),
            supported_formats: vec!["fvp.hcb".into()].into(),
        }
    }

    fn report(family_id: &str, confidence_permyriad: u16) -> ProbeReport {
        ProbeReport {
            family_id: family_id.into(),
            game_id: "game".into(),
            format: "fvp.hcb".into(),
            confidence_permyriad,
        }
    }

    #[test]
    fn static_providers_use_same_multi_probe_policy() {
        let mut registry = FamilyProviderRegistry::new();
        registry
            .register_provider(ProbeProvider {
                descriptor: descriptor("a", "fvp"),
                report: Some(report("fvp", 8_000)),
            })
            .unwrap();
        registry
            .register_provider(ProbeProvider {
                descriptor: descriptor("b", "fvp"),
                report: Some(report("fvp", 9_000)),
            })
            .unwrap();
        let selection = registry
            .probe(
                &ProbeRequest {
                    game_path: "game".into(),
                },
                None,
                None,
            )
            .unwrap();
        assert!(selection.requires_user_choice());
        assert_eq!(selection.candidates()[0].report.plugin_id, "b");
    }
}
