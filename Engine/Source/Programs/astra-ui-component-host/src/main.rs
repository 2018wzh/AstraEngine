use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use abi_stable::library::{AbiHeaderRef, ROOT_MODULE_LOADER_NAME_WITH_NUL};
use astra_ui_plugin_abi::{
    UiComponentError, UiComponentFrame, UiComponentManifest, UiComponentModuleRef,
    UiComponentRequest, UiComponentResponse,
};
use libloading::Library;

const MAX_CALL_TIMEOUT_NS: u64 = 60_000_000_000;

fn main() {
    if let Err(error) = run() {
        tracing::error!(
            event = "ui.component_host.failed",
            code = error_code(&error),
            "UI component host terminated"
        );
        std::process::exit(2);
    }
}

fn run() -> Result<(), UiComponentError> {
    let arguments = Arguments::parse(std::env::args_os().skip(1))?;
    let artifact = fs::read(&arguments.artifact)?;
    let manifest_bytes = fs::read(&arguments.manifest)?;
    let allowlist_bytes = fs::read(&arguments.allowlist)?;
    let manifest: UiComponentManifest = postcard::from_bytes(&manifest_bytes)
        .map_err(|error| UiComponentError::Codec(error.to_string()))?;
    let allowlist: BTreeMap<String, [u8; 32]> = postcard::from_bytes(&allowlist_bytes)
        .map_err(|error| UiComponentError::Codec(error.to_string()))?;
    manifest.verify(&artifact, &allowlist)?;

    let library = unsafe { Library::new(&arguments.artifact) }.map_err(|_| {
        UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_LOAD: signed component library could not be loaded".to_string(),
        )
    })?;
    let module = unsafe { root_module(&library)? };
    let embedded_bytes = (module.manifest_postcard())().to_vec();
    let embedded: UiComponentManifest = postcard::from_bytes(&embedded_bytes)
        .map_err(|error| UiComponentError::Codec(error.to_string()))?;
    if embedded != manifest {
        return Err(UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_EMBEDDED_MANIFEST: embedded manifest differs from signed sidecar"
                .to_string(),
        ));
    }

    let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>();
    std::thread::spawn(move || worker_loop(library, module, request_rx));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    let mut expected_sequence = 1u64;
    loop {
        let frame = UiComponentFrame::decode(&mut input)?;
        if frame.sequence != expected_sequence {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_SEQUENCE: request sequence is not contiguous".to_string(),
            ));
        }
        expected_sequence = expected_sequence.saturating_add(1);
        let request: UiComponentRequest = postcard::from_bytes(&frame.payload)
            .map_err(|error| UiComponentError::Codec(error.to_string()))?;
        request.validate()?;
        let shutdown = matches!(request, UiComponentRequest::Shutdown);
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        request_tx
            .send(WorkerRequest {
                kind: frame.kind,
                payload: frame.payload,
                response: response_tx,
            })
            .map_err(|_| {
                UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_PROCESS_EXIT: component worker exited".to_string(),
                )
            })?;
        let timeout = Duration::from_nanos(frame.deadline_ns.min(MAX_CALL_TIMEOUT_NS));
        let response_payload = match response_rx.recv_timeout(timeout) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::error!(
                    event = "ui.component_host.timeout",
                    sequence = frame.sequence,
                    "UI component call exceeded deadline"
                );
                std::process::exit(124);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_PROCESS_EXIT: component worker disconnected".to_string(),
                ));
            }
        };
        let response: UiComponentResponse = postcard::from_bytes(&response_payload)
            .map_err(|error| UiComponentError::Codec(error.to_string()))?;
        response.validate()?;
        UiComponentFrame {
            kind: frame.kind | 0x8000,
            sequence: frame.sequence,
            deadline_ns: frame.deadline_ns,
            payload: response_payload,
        }
        .encode(&mut output)?;
        if shutdown {
            break;
        }
    }
    Ok(())
}

struct WorkerRequest {
    kind: u16,
    payload: Vec<u8>,
    response: mpsc::SyncSender<Result<Vec<u8>, UiComponentError>>,
}

fn worker_loop(_library: Library, module: UiComponentModuleRef, rx: mpsc::Receiver<WorkerRequest>) {
    for request in rx {
        let invoke = match request.kind {
            1 => module.create(),
            2 => module.frame(),
            3 => module.snapshot(),
            4 => module.restore(),
            5 => module.shutdown(),
            _ => {
                let _ = request.response.send(Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_REQUEST_KIND: unknown request kind".to_string(),
                )));
                continue;
            }
        };
        let result = invoke(request.payload.into());
        let response = if result.ok {
            Ok(result.payload.to_vec())
        } else {
            Err(UiComponentError::Invalid(format!(
                "ASTRA_UI_COMPONENT_INVOKE: {}",
                result.diagnostic
            )))
        };
        let _ = request.response.send(response);
    }
}

unsafe fn root_module(library: &Library) -> Result<UiComponentModuleRef, UiComponentError> {
    let header = library
        .get::<AbiHeaderRef>(ROOT_MODULE_LOADER_NAME_WITH_NUL.as_bytes())
        .map_err(|_| {
            UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_ABI_SYMBOL: root module symbol is missing".to_string(),
            )
        })?;
    let header = (*header).upgrade().map_err(|_| {
        UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_ABI_HEADER: component ABI header is incompatible".to_string(),
        )
    })?;
    header
        .init_root_module::<UiComponentModuleRef>()
        .map_err(|_| {
            UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_ABI_FINGERPRINT: component ABI is incompatible".to_string(),
            )
        })
}

struct Arguments {
    manifest: PathBuf,
    artifact: PathBuf,
    allowlist: PathBuf,
}

impl Arguments {
    fn parse(
        arguments: impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<Self, UiComponentError> {
        let mut manifest = None;
        let mut artifact = None;
        let mut allowlist = None;
        let mut arguments = arguments;
        while let Some(name) = arguments.next() {
            let value = arguments.next().ok_or_else(|| {
                UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_ARGUMENT: option requires a value".to_string(),
                )
            })?;
            match name.to_str() {
                Some("--manifest") => manifest = Some(PathBuf::from(value)),
                Some("--artifact") => artifact = Some(PathBuf::from(value)),
                Some("--allowlist") => allowlist = Some(PathBuf::from(value)),
                _ => {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_ARGUMENT: unknown option".to_string(),
                    ));
                }
            }
        }
        let required = |value: Option<PathBuf>, name: &str| {
            value.ok_or_else(|| {
                UiComponentError::Invalid(format!(
                    "ASTRA_UI_COMPONENT_ARGUMENT: missing required option {name}"
                ))
            })
        };
        let result = Self {
            manifest: required(manifest, "--manifest")?,
            artifact: required(artifact, "--artifact")?,
            allowlist: required(allowlist, "--allowlist")?,
        };
        for path in [&result.manifest, &result.artifact, &result.allowlist] {
            validate_path(path)?;
        }
        Ok(result)
    }
}

fn validate_path(path: &Path) -> Result<(), UiComponentError> {
    if !path.is_file() {
        return Err(UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_INPUT_MISSING: component input does not exist".to_string(),
        ));
    }
    Ok(())
}

fn error_code(error: &UiComponentError) -> &'static str {
    match error {
        UiComponentError::Invalid(_) => "ASTRA_UI_COMPONENT_INVALID",
        UiComponentError::Io(_) => "ASTRA_UI_COMPONENT_IO",
        UiComponentError::Codec(_) => "ASTRA_UI_COMPONENT_CODEC",
        UiComponentError::Signature => "ASTRA_UI_COMPONENT_SIGNATURE",
    }
}
