//! Family lifecycle over the private process transport. No native state is
//! shared between sessions, and no Host device or window handle enters it.
mod frame;
mod input;
mod runtime;
mod session;

use abi_stable::{
    prefix_type::PrefixTypeTrait, sabi_types::Constructor, std_types::ROption,
    type_level::downcasting::TD_Opaque,
};
use astra_emu_family_api::*;
use std::{
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock,
    },
};

fn error(code: &str, message: &str) -> FamilyError {
    FamilyError::invalid(code, message)
}
fn transport_error(operation: &str) -> FamilyError {
    error(operation, "KrKr native session operation failed")
}
static ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static DIAGNOSTICS: OnceLock<FamilyResult<Arc<DiagnosticSinkBox>>> = OnceLock::new();

struct Lease;
impl Lease {
    fn acquire() -> FamilyResult<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_KRKR_SESSION_ACTIVE",
                    "close the active KrKr session first",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

pub fn krkr_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        family_id: "krkr".into(),
        plugin_id: "astra.emu.krkr".into(),
        abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
        ]
        .into(),
        supported_formats: vec!["krkr.xp3".into()].into(),
        configuration: Vec::new().into(),
    }
}

#[derive(Default)]
pub struct KrkrProvider;
impl FamilyProvider for KrkrProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let descriptor = krkr_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    }
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate()?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        let entries = std::fs::read_dir(root)
            .map_err(|_| error("ASTRA_EMU_KRKR_PROBE", "game directory could not be read"))?;
        let mut matched = false;
        for (index, entry) in entries.enumerate() {
            if index == 4096 {
                return Err(error(
                    "ASTRA_EMU_KRKR_PROBE_BOUND",
                    "game directory exceeds discovery bound",
                ));
            }
            let entry = entry
                .map_err(|_| error("ASTRA_EMU_KRKR_PROBE", "game directory could not be read"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xp3"))
            {
                continue;
            }
            let Ok(path) = astra_emu_sdk::resolve_game_file(root, name) else {
                continue;
            };
            let mut magic = [0; 11];
            if std::fs::File::open(path)
                .and_then(|mut f| f.read_exact(&mut magic))
                .is_ok()
                && &magic == b"XP3\r\n \n\x1a\x8bg\x01"
            {
                matched = true;
            }
        }
        if !matched {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| error("ASTRA_EMU_KRKR_GAME_ID", "game ID is not valid UTF-8"))?;
        let report = ProbeReport {
            family_id: "krkr".into(),
            game_id: game_id.into(),
            format: "krkr.xp3".into(),
            confidence_permyriad: 9000,
        };
        report.validate()?;
        Ok(Some(report))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        request.validate_for_descriptor(&self.descriptor()?)?;
        let game = runtime::game_directory(Path::new(request.game_path.as_str()))?;
        let executable = runtime::child_executable()?;
        let diagnostics = DIAGNOSTICS
            .get()
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_KRKR_DIAGNOSTICS",
                    "initialize host diagnostics before opening KrKr",
                )
            })?
            .clone()?;
        let sink = match request.host.audio_sink {
            ROption::RSome(sink) => Arc::new(sink),
            ROption::RNone => {
                return Err(error(
                    "ASTRA_EMU_KRKR_AUDIO",
                    "KrKr requires the host audio sink",
                ))
            }
        };
        let mut id = NEXT_SESSION.load(Ordering::Acquire);
        loop {
            let next = id
                .checked_add(1)
                .ok_or_else(|| error("ASTRA_EMU_KRKR_SESSION_ID", "session ID exhausted"))?;
            match NEXT_SESSION.compare_exchange_weak(id, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(current) => id = current,
            }
        }
        let lease = Lease::acquire()?;
        session::open(
            executable,
            game,
            request.initial_window,
            sink,
            diagnostics,
            id,
            lease,
        )
    }
}

struct SharedDiagnostics(Arc<DiagnosticSinkBox>);
impl DiagnosticSink for SharedDiagnostics {
    fn enabled(&self, level: DiagnosticLevel) -> bool {
        self.0.enabled(level)
    }
    fn emit(&self, event: DiagnosticEvent) {
        self.0.emit(event);
    }
}
#[derive(Default)]
struct KrkrModule(ProviderModule<KrkrProvider>);
impl FamilyModule for KrkrModule {
    fn initialize_diagnostics(&self, sink: DiagnosticSinkBox) -> FfiFamilyResult<()> {
        let result = DIAGNOSTICS.get_or_init(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let sink = Arc::new(sink);
                diagnostic_bridge::install(DiagnosticSink_TO::from_value(
                    SharedDiagnostics(Arc::clone(&sink)),
                    TD_Opaque,
                ))?;
                Ok(sink)
            }))
            .unwrap_or_else(|_| {
                Err(error(
                    "ASTRA_EMU_KRKR_DIAGNOSTICS",
                    "host diagnostic initialization failed",
                ))
            })
        });
        result.as_ref().map(|_| ()).map_err(Clone::clone).into()
    }
    fn descriptor(&self) -> FfiFamilyResult<FamilyDescriptor> {
        self.0.descriptor()
    }
    fn probe(&self, request: ProbeRequest) -> FfiFamilyResult<ROption<ProbeReport>> {
        self.0.probe(request)
    }
    fn open(&self, request: OpenRequest) -> FfiFamilyResult<OpenResponse> {
        self.0.open(request)
    }
    fn advance(&self, request: AdvanceRequest) -> FfiFamilyResult<AdvanceResponse> {
        self.0.advance(request)
    }
    fn frame(
        &self,
        request: SessionRequest,
        consumer: FrameConsumerRef<'_>,
    ) -> FfiFamilyResult<()> {
        self.0.frame(request, consumer)
    }
    fn close(&self, request: SessionRequest) -> FfiFamilyResult<()> {
        self.0.close(request)
    }
}
extern "C" fn construct_module() -> FamilyModuleBox {
    FamilyModule_TO::from_value(KrkrModule::default(), TD_Opaque)
}
#[abi_stable::export_root_module]
pub fn astra_krkr_family_root_module() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct_module),
    }
    .leak_into_prefix()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_requires_native_xp3_magic() {
        let dir = tempfile::tempdir().unwrap();
        let request = || ProbeRequest {
            game_path: dir.path().to_string_lossy().into_owned().into(),
        };
        let provider = KrkrProvider;
        assert!(provider.probe(request()).unwrap().is_none());
        std::fs::write(dir.path().join("any.xp3"), b"not an archive").unwrap();
        assert!(provider.probe(request()).unwrap().is_none());
        std::fs::write(dir.path().join("any.xp3"), b"XP3\r\n \n\x1a\x8bg\x01").unwrap();
        assert_eq!(
            provider.probe(request()).unwrap().unwrap().format,
            "krkr.xp3"
        );
        krkr_descriptor().validate().unwrap();
    }
}
