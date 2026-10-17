//! Native C protocol loader. A loaded library is resident even if validation fails.
use crate::{family::FamilyPluginDescriptor, model::*};
use astra_emu_abi as abi;
use std::{
    mem::{ManuallyDrop, MaybeUninit},
    path::{Path, PathBuf},
    rc::Rc,
};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FamilyLoadError {
    #[error("ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT")]
    Diagnostics,
    #[error("ASTRA_EMU_FAMILY_SCAN_DIRECTORY")]
    Directory,
    #[error("ASTRA_EMU_FAMILY_LOAD_PATH")]
    InvalidPath,
    #[error("ASTRA_EMU_FAMILY_LOAD_LIBRARY")]
    Library,
    #[error("ASTRA_EMU_FAMILY_LOAD_ABI")]
    AbiLayout,
    #[error("ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR")]
    Descriptor,
    #[error("{0}")]
    DescriptorError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN")]
    DuplicatePlugin,
    #[error("ASTRA_EMU_FAMILY_LOAD_PROVIDER")]
    Provider,
    #[error("{0}")]
    ProviderError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_PROBE")]
    Probe,
    #[error("{0}")]
    ProbeError(FamilyError),
    #[error("ASTRA_EMU_FAMILY_LOAD_POLICY")]
    Policy,
    #[error("{0}")]
    PolicyError(String),
}
impl FamilyLoadError {
    pub fn diagnostic_code(&self) -> &str {
        match self {
            Self::Diagnostics => "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT",
            Self::Directory => "ASTRA_EMU_FAMILY_SCAN_DIRECTORY",
            Self::InvalidPath => "ASTRA_EMU_FAMILY_LOAD_PATH",
            Self::Library => "ASTRA_EMU_FAMILY_LOAD_LIBRARY",
            Self::AbiLayout => "ASTRA_EMU_FAMILY_LOAD_ABI",
            Self::Descriptor => "ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR",
            Self::DuplicatePlugin => "ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN",
            Self::Provider => "ASTRA_EMU_FAMILY_LOAD_PROVIDER",
            Self::Probe => "ASTRA_EMU_FAMILY_LOAD_PROBE",
            Self::Policy | Self::PolicyError(_) => "ASTRA_EMU_FAMILY_LOAD_POLICY",
            Self::DescriptorError(e) | Self::ProviderError(e) | Self::ProbeError(e) => e.code(),
        }
    }
    pub fn human_message(&self) -> &str {
        match self {
            Self::DescriptorError(e) | Self::ProviderError(e) | Self::ProbeError(e) => &e.message,
            Self::PolicyError(e) => e,
            Self::DuplicatePlugin => "all cores with this duplicate plugin ID are disabled",
            Self::Diagnostics => "core diagnostic bridge initialization failed",
            _ => self.diagnostic_code(),
        }
    }
}

pub(crate) fn checked(status: abi::Status, error: abi::Error) -> FamilyResult<()> {
    if status == abi::OK {
        return Ok(());
    }
    let code = std::str::from_utf8(
        error
            .code
            .get(..error.code_len as usize)
            .unwrap_or_default(),
    )
    .unwrap_or("ASTRA_EMU_CORE_ERROR");
    let message = std::str::from_utf8(
        error
            .message
            .get(..error.message_len as usize)
            .unwrap_or_default(),
    )
    .unwrap_or("invalid core diagnostic encoding");
    Err(FamilyError::new(
        if code.is_empty() {
            "ASTRA_EMU_CORE_ERROR"
        } else {
            code
        },
        message,
    ))
}
pub(crate) unsafe fn text(bytes: abi::Bytes, limit: usize) -> FamilyResult<String> {
    unsafe { bytes.text(limit) }
        .map(str::to_owned)
        .map_err(|_| {
            FamilyError::invalid("ASTRA_EMU_CORE_METADATA", "invalid bounded UTF-8 metadata")
        })
}

struct NativeModule {
    api: abi::CoreApi,
    resources: Option<crate::native_module_owner::ModuleResources>,
    d3d12: Option<abi::D3D12Api>,
    closed: bool,
}
pub struct LoadedFamilyPlugin {
    inner: Rc<NativeModule>,
    descriptor: FamilyPluginDescriptor,
    configuration: Vec<ConfigField>,
    location: PathBuf,
}
impl LoadedFamilyPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FamilyLoadError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(FamilyLoadError::InvalidPath);
        }
        // Deliberately resident on every subsequent failure path as well.
        #[cfg(target_os = "windows")]
        let loaded = {
            use libloading::os::windows::{
                Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            };
            let absolute = std::fs::canonicalize(path).map_err(|_| FamilyLoadError::InvalidPath)?;
            // A core's app-local dependencies live beside its DLL, not beside
            // Manager or in the process working directory. Do not mutate PATH.
            unsafe {
                Library::load_with_flags(
                    absolute,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
                )
            }
            .map(libloading::Library::from)
        };
        #[cfg(not(target_os = "windows"))]
        let loaded = unsafe { libloading::Library::new(path) };
        let library = ManuallyDrop::new(loaded.map_err(|_| FamilyLoadError::Library)?);
        let query = unsafe { library.get::<abi::Query>(abi::QUERY_SYMBOL) }
            .map_err(|_| FamilyLoadError::AbiLayout)?;
        let mut core = unsafe { Self::from_query(*query) }?;
        core.location = path.to_owned();
        Ok(core)
    }
    /// # Safety
    /// The query and every function/storage it returns must remain valid until process exit.
    pub unsafe fn from_query(query: abi::Query) -> Result<Self, FamilyLoadError> {
        let mut table = std::ptr::null();
        if unsafe { query(abi::ABI_VERSION, &mut table) } != abi::OK
            || table.is_null()
            || !table.is_aligned()
        {
            return Err(FamilyLoadError::AbiLayout);
        }
        // Read the prefix before copying the full table.
        if !unsafe { (*table).header }.accepts::<abi::CoreApi>() {
            return Err(FamilyLoadError::AbiLayout);
        }
        let api = unsafe { *table };
        api.validate().map_err(|_| FamilyLoadError::AbiLayout)?;
        let mut error = abi::Error::default();
        checked(
            unsafe { api.initialize.unwrap()(crate::family_diagnostics::sink(), &mut error) },
            error,
        )
        .map_err(|_| FamilyLoadError::Diagnostics)?;
        let mut descriptor = MaybeUninit::uninit();
        let mut error = abi::Error::default();
        checked(
            unsafe { api.describe.unwrap()(descriptor.as_mut_ptr(), &mut error) },
            error,
        )
        .map_err(FamilyLoadError::DescriptorError)?;
        let (descriptor, configuration) =
            unsafe { super::native_config::descriptor(descriptor.assume_init()) }
                .map_err(FamilyLoadError::DescriptorError)?;
        let resources = crate::native_module_owner::ModuleResources::discover(&api)
            .map_err(FamilyLoadError::ProviderError)?;
        let mut extension = std::ptr::null();
        let status = unsafe {
            api.extension.unwrap()(
                abi::Bytes::new(abi::D3D12_EXTENSION),
                abi::D3D12_EXTENSION_VERSION,
                &mut extension,
            )
        };
        let d3d12 = if status == abi::UNSUPPORTED && extension.is_null() {
            None
        } else {
            if status != abi::OK
                || extension.is_null()
                || !extension.cast::<abi::D3D12Api>().is_aligned()
            {
                return Err(FamilyLoadError::AbiLayout);
            }
            let header = unsafe { *extension.cast::<abi::Header>() };
            if header.version != abi::D3D12_EXTENSION_VERSION
                || header.size < abi::D3D12Api::HEADER.size
            {
                return Err(FamilyLoadError::AbiLayout);
            }
            let table = unsafe { *extension.cast::<abi::D3D12Api>() };
            table.validate().map_err(|_| FamilyLoadError::AbiLayout)?;
            Some(table)
        };
        Ok(Self {
            inner: Rc::new(NativeModule {
                api,
                resources,
                d3d12,
                closed: false,
            }),
            descriptor,
            configuration,
            location: PathBuf::new(),
        })
    }
    pub fn supports_d3d12(&self) -> bool {
        self.inner.d3d12.is_some()
    }
    pub fn location(&self) -> &Path {
        &self.location
    }
    pub fn manager_descriptor(&self) -> &FamilyPluginDescriptor {
        &self.descriptor
    }
    pub fn configuration(&self) -> &[ConfigField] {
        &self.configuration
    }
    /// Consume cross-session resources only when every native session has
    /// released its module reference. The dynamic library remains resident.
    pub fn shutdown(&mut self) -> FamilyResult<()> {
        let module = Rc::get_mut(&mut self.inner).ok_or_else(|| {
            FamilyError::invalid(
                "ASTRA_EMU_MODULE_OWNER_BUSY",
                "native sessions still retain this module",
            )
        })?;
        if module.closed {
            return Ok(());
        }
        module.closed = true;
        module
            .resources
            .take()
            .map_or(Ok(()), |mut resources| resources.close())
    }
    pub fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        if self.inner.closed {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_MODULE_CLOSED",
                "native module resources have been closed",
            ));
        }
        request.validate()?;
        let mut out = MaybeUninit::uninit();
        let mut error = abi::Error::default();
        let status = unsafe {
            self.inner.api.probe.unwrap()(
                abi::Bytes::new(request.game_path.as_bytes()),
                out.as_mut_ptr(),
                &mut error,
            )
        };
        if status == abi::NO_MATCH {
            return Ok(None);
        }
        checked(status, error)?;
        let out: abi::ProbeResult = unsafe { out.assume_init() };
        let report = ProbeReport {
            family_id: self.descriptor.family_id.clone(),
            game_id: unsafe { text(out.game_id, 256) }?,
            format: unsafe { text(out.format, 256) }?,
            confidence_permyriad: u16::try_from(out.confidence_permyriad).map_err(|_| {
                FamilyError::invalid("ASTRA_EMU_PROBE_CONFIDENCE", "invalid probe confidence")
            })?,
        };
        report.validate()?;
        if !self.descriptor.supported_formats.contains(&report.format) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_PROBE_FORMAT",
                "undeclared format",
            ));
        }
        Ok(Some(report))
    }
    pub fn open(&mut self, request: NativeOpenRequest) -> FamilyResult<NativeSession> {
        if self.inner.closed {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_MODULE_CLOSED",
                "native module resources have been closed",
            ));
        }
        request.initial_window.validate()?;
        if request.session_epoch == 0
            || request.cancellation.epoch() != request.session_epoch
            || request.cancellation.is_cancelled()
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_CANCELLED",
                "open context cancelled or invalid",
            ));
        }
        crate::model::validate_game_path(&request.game_path)?;
        let resolved = resolve_config(&self.configuration, &request.configuration)?;
        let entries = super::native_config::entries(&resolved);
        let raw = abi::OpenRequest {
            header: abi::Header::of::<abi::OpenRequest>(),
            game_path: abi::Bytes::new(request.game_path.as_bytes()),
            session_epoch: request.session_epoch,
            configuration: entries.as_ptr(),
            configuration_count: entries.len(),
            window: native_window(request.initial_window),
            host: request.host,
            module_owner: self
                .inner
                .resources
                .as_ref()
                .map_or(std::ptr::null_mut(), |resources| resources.handle()),
        };
        let mut output = MaybeUninit::<abi::OpenResult>::zeroed();
        let mut error = abi::Error::default();
        let status = unsafe {
            if let Some(gpu) = self.inner.d3d12 {
                gpu.open.unwrap()(
                    &raw,
                    request.d3d12.as_ref().map_or(std::ptr::null(), |host| host),
                    output.as_mut_ptr(),
                    &mut error,
                )
            } else {
                self.inner.api.open.unwrap()(&raw, output.as_mut_ptr(), &mut error)
            }
        };
        let output = unsafe { output.assume_init() };
        if status != abi::OK {
            if !output.session.is_null() {
                unsafe {
                    self.inner.api.cancel.unwrap()(output.session);
                    self.inner.api.close.unwrap()(output.session, &mut abi::Error::default());
                }
            }
            return checked(status, error).and_then(|_| unreachable!());
        }
        let session = NativeSession {
            inner: Some(Rc::clone(&self.inner)),
            handle: output.session,
            session_epoch: request.session_epoch,
            window: raw.window,
            pointer: (0.0, 0.0),
            poisoned: false,
            cancellation: request.cancellation,
            d3d12: if output.surface.kind == abi::SURFACE_GPU {
                self.inner.d3d12.zip(request.d3d12)
            } else {
                None
            },
        };
        if !session.cancellation.publish() {
            drop(session);
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_CANCELLED",
                "open cancelled before handle publication",
            ));
        }
        if output.session.is_null()
            || output.context_epoch == 0
            || output.output_epoch == 0
            || output.surface.validate().is_err()
            || (output.surface.kind == abi::SURFACE_GPU
                && (output.surface.backend != abi::BACKEND_D3D12 || session.d3d12.is_none()))
        {
            drop(session);
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_OPEN_RESULT",
                "invalid or unsupported surface",
            ));
        }
        Ok(session)
    }
}
pub struct NativeOpenRequest {
    pub game_path: String,
    pub session_epoch: u64,
    pub configuration: Vec<ConfigEntry>,
    pub initial_window: WindowState,
    pub host: abi::HostServices,
    pub cancellation: crate::NativeCancellation,
    pub d3d12: Option<abi::D3D12Host>,
}
pub fn no_host_services(cancellation: &crate::NativeCancellation) -> abi::HostServices {
    abi::HostServices {
        header: abi::Header::of::<abi::HostServices>(),
        text: abi::TextService {
            context: std::ptr::null_mut(),
            request: None,
            acknowledge: None,
            invalidate: None,
        },
        window: abi::WindowService {
            context: std::ptr::null_mut(),
            request: None,
            invalidate: None,
        },
        cancellation: cancellation.service(),
    }
}
fn native_window(state: WindowState) -> abi::WindowState {
    abi::WindowState {
        drawable_width: state.width,
        drawable_height: state.height,
        focused: state.focused as u32,
        suspended: (!state.visible) as u32,
        scale_factor: state.scale_factor,
    }
}

pub struct NativeSession {
    inner: Option<Rc<NativeModule>>,
    handle: abi::Session,
    session_epoch: u64,
    window: abi::WindowState,
    pointer: (f64, f64),
    poisoned: bool,
    cancellation: crate::NativeCancellation,
    d3d12: Option<(abi::D3D12Api, abi::D3D12Host)>,
}
impl NativeSession {
    fn call(
        &mut self,
        call: impl FnOnce(abi::CoreApi, abi::Session, &mut abi::Error) -> abi::Status,
    ) -> FamilyResult<()> {
        if self.handle.is_null() || self.poisoned {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_SESSION_UNAVAILABLE",
                "session is closed or poisoned",
            ));
        }
        let mut error = abi::Error::default();
        let status = call(
            self.inner
                .as_ref()
                .expect("live session retains module")
                .api,
            self.handle,
            &mut error,
        );
        self.poisoned |= status == abi::PANICKED;
        checked(status, error)
    }
    pub fn input(
        &mut self,
        events: &[FamilyEvent],
        displayed: Option<FrameIdentity>,
    ) -> FamilyResult<()> {
        crate::model::validate_events(events)?;
        for event in events {
            let mut raw = abi::InputEvent {
                kind: 0,
                pressed: 0,
                repeat: 0,
                button: 0,
                physical_key: abi::Bytes::EMPTY,
                logical_key: abi::Bytes::EMPTY,
                x: self.pointer.0,
                y: self.pointer.1,
                displayed: displayed.map(Into::into).unwrap_or_default(),
                window: self.window,
            };
            match event {
                FamilyEvent::Control { command } => {
                    self.control(*command)?;
                    continue;
                }
                FamilyEvent::Key {
                    code,
                    state,
                    logical,
                    ..
                } => {
                    raw.kind = abi::INPUT_KEY;
                    raw.physical_key = abi::Bytes::new(code.physical_name().as_bytes());
                    raw.logical_key = abi::Bytes::new(
                        logical
                            .as_deref()
                            .unwrap_or_else(|| code.logical_name())
                            .as_bytes(),
                    );
                    raw.pressed = (*state != KeyState::Released) as u32;
                    raw.repeat = (*state == KeyState::Repeated) as u32;
                }
                FamilyEvent::TextInput { text } => {
                    raw.kind = abi::INPUT_KEY;
                    raw.logical_key = abi::Bytes::new(text.as_bytes());
                    raw.pressed = 1;
                }
                FamilyEvent::PointerMove {
                    x,
                    y,
                    displayed: event_frame,
                } => {
                    self.pointer = (f64::from(*x), f64::from(*y));
                    raw.kind = abi::INPUT_POINTER_MOVE;
                    raw.x = self.pointer.0;
                    raw.y = self.pointer.1;
                    raw.displayed = event_frame
                        .or(displayed)
                        .map(Into::into)
                        .unwrap_or_default();
                }
                FamilyEvent::PointerButton {
                    button,
                    state,
                    displayed: event_frame,
                } => {
                    raw.kind = abi::INPUT_POINTER_BUTTON;
                    raw.button = match button {
                        PointerButton::Primary => abi::BUTTON_PRIMARY,
                        PointerButton::Secondary => abi::BUTTON_SECONDARY,
                        PointerButton::Middle => abi::BUTTON_MIDDLE,
                        PointerButton::Back => 4,
                        PointerButton::Forward => 5,
                    };
                    raw.pressed = (*state != KeyState::Released) as u32;
                    raw.displayed = event_frame
                        .or(displayed)
                        .map(Into::into)
                        .unwrap_or_default();
                }
                FamilyEvent::Wheel { delta_x, delta_y } => {
                    raw.kind = abi::INPUT_WHEEL;
                    raw.x = f64::from(*delta_x);
                    raw.y = f64::from(*delta_y);
                }
                FamilyEvent::WindowFocused { focused } => {
                    self.window.focused = *focused as u32;
                    raw.kind = abi::INPUT_WINDOW;
                }
                FamilyEvent::WindowResized { width, height } => {
                    self.window.drawable_width = *width;
                    self.window.drawable_height = *height;
                    raw.kind = abi::INPUT_WINDOW;
                }
                FamilyEvent::WindowScale { factor } => {
                    self.window.scale_factor = *factor;
                    raw.kind = abi::INPUT_WINDOW;
                }
                FamilyEvent::WindowVisibility { visible } => {
                    self.window.suspended = (!visible) as u32;
                    raw.kind = abi::INPUT_WINDOW;
                }
                FamilyEvent::WindowSuspended { suspended } => {
                    self.window.suspended = *suspended as u32;
                    raw.kind = abi::INPUT_WINDOW;
                }
                FamilyEvent::WindowCloseRequested => {
                    self.cancel();
                    continue;
                }
            }
            raw.window = self.window;
            self.call(|api, handle, error| unsafe { api.input.unwrap()(handle, &raw, 1, error) })?;
        }
        Ok(())
    }
    pub fn state(&mut self) -> FamilyResult<abi::SessionState> {
        let mut state = MaybeUninit::uninit();
        self.call(|api, handle, error| unsafe {
            api.state.unwrap()(handle, state.as_mut_ptr(), error)
        })?;
        let state = unsafe { state.assume_init() };
        state.validate().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_CORE_STATE",
                "native state or retry flags are invalid",
            )
        })?;
        Ok(state)
    }
    pub fn control(&mut self, control: u32) -> FamilyResult<()> {
        self.call(|api, handle, error| unsafe { api.control.unwrap()(handle, control, error) })
    }
    pub fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
        if let Some((api, host)) = self.d3d12 {
            let mut frame = MaybeUninit::<abi::D3D12Frame>::zeroed();
            let result = self.call(|_, handle, error| unsafe {
                api.acquire.unwrap()(
                    handle,
                    after.map(Into::into).unwrap_or_default(),
                    frame.as_mut_ptr(),
                    error,
                )
            });
            let frame = unsafe { frame.assume_init() };
            if let Err(error) = result {
                if let Some(release) = frame.lease.release {
                    unsafe {
                        release(frame.lease.owner, abi::D3D12_NOT_SUBMITTED);
                    }
                }
                return Err(error);
            }
            return unsafe {
                super::native_frame::take_d3d12(frame, &host, self.session_epoch, after)
            };
        }
        let mut surface = MaybeUninit::<abi::FrameSurface>::zeroed();
        let result = self.call(|api, handle, error| unsafe {
            api.frame.unwrap()(
                handle,
                after.map(Into::into).unwrap_or_default(),
                surface.as_mut_ptr(),
                error,
            )
        });
        let surface = unsafe { surface.assume_init() };
        if let Err(error) = result {
            if let Some(release) = surface.cpu.release {
                unsafe { release(surface.cpu.owner) };
            }
            return Err(error);
        }
        unsafe { super::native_frame::take_surface(surface, self.session_epoch, after) }
    }
    pub fn cancel(&self) {
        self.cancellation.cancel();
        if !self.handle.is_null() {
            unsafe {
                self.inner
                    .as_ref()
                    .expect("live session retains module")
                    .api
                    .cancel
                    .unwrap()(self.handle)
            };
        }
    }
    pub fn close(&mut self) -> FamilyResult<()> {
        self.cancellation.cancel();
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        let module = self.inner.take();
        if handle.is_null() {
            return Ok(());
        }
        let module = module.expect("live session retains module");
        unsafe { module.api.cancel.unwrap()(handle) };
        let mut error = abi::Error::default();
        checked(
            unsafe { module.api.close.unwrap()(handle, &mut error) },
            error,
        )
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "astra.emu.core.close_failed", code = %error.code());
        }
    }
}
