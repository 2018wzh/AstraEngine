//! Parent-side process isolation for signed UI components.

use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use astra_ui_plugin_abi::{
    UiComponentError, UiComponentFrame, UiComponentRequest, UiComponentResponse,
};

const RESPONSE_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct UiComponentProcessConfig {
    pub host_binary: PathBuf,
    pub manifest: PathBuf,
    pub artifact: PathBuf,
    pub allowlist: PathBuf,
    pub deadline: Duration,
}

impl UiComponentProcessConfig {
    pub fn validate(&self) -> Result<(), UiComponentError> {
        for (label, path) in [
            ("host", &self.host_binary),
            ("manifest", &self.manifest),
            ("artifact", &self.artifact),
            ("allowlist", &self.allowlist),
        ] {
            if !path.is_file() {
                return Err(UiComponentError::Invalid(format!(
                    "ASTRA_UI_COMPONENT_PROCESS_INPUT: {label} input is not a file"
                )));
            }
        }
        if self.deadline.is_zero() || self.deadline > Duration::from_secs(60) {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_DEADLINE: deadline must be within 1ns..=60s".into(),
            ));
        }
        Ok(())
    }
}

pub struct UiComponentProcess {
    child: Child,
    input: BufWriter<ChildStdin>,
    responses: mpsc::Receiver<Result<UiComponentFrame, UiComponentError>>,
    next_sequence: u64,
    deadline: Duration,
    terminated: bool,
}

impl UiComponentProcess {
    pub fn spawn(config: UiComponentProcessConfig) -> Result<Self, UiComponentError> {
        config.validate()?;
        let mut command = Command::new(&config.host_binary);
        command
            .arg("--manifest")
            .arg(&config.manifest)
            .arg("--artifact")
            .arg(&config.artifact)
            .arg("--allowlist")
            .arg(&config.allowlist)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        configure_hidden_process(&mut command);
        let mut child = command.spawn().map_err(UiComponentError::Io)?;
        let input = child.stdin.take().ok_or_else(|| {
            UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_STDIN: child stdin unavailable".into(),
            )
        })?;
        let output = child.stdout.take().ok_or_else(|| {
            UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_STDOUT: child stdout unavailable".into(),
            )
        })?;
        let (response_tx, responses) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("astra-ui-component-response".into())
            .spawn(move || {
                let mut output = BufReader::new(output);
                loop {
                    let response = UiComponentFrame::decode(&mut output);
                    let terminal = response.is_err();
                    if response_tx.send(response).is_err() || terminal {
                        break;
                    }
                }
            })
            .map_err(UiComponentError::Io)?;
        Ok(Self {
            child,
            input: BufWriter::new(input),
            responses,
            next_sequence: 1,
            deadline: config.deadline,
            terminated: false,
        })
    }

    pub fn invoke(
        &mut self,
        request: UiComponentRequest,
    ) -> Result<UiComponentResponse, UiComponentError> {
        if self.terminated {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_TERMINATED: component session is terminated".into(),
            ));
        }
        request.validate()?;
        let kind = request_kind(&request);
        let payload = postcard::to_allocvec(&request)
            .map_err(|error| UiComponentError::Codec(error.to_string()))?;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_SEQUENCE: sequence exhausted".into(),
            )
        })?;
        let deadline_ns = self.deadline.as_nanos().min(u64::MAX as u128) as u64;
        if let Err(error) = (UiComponentFrame {
            kind,
            sequence,
            deadline_ns,
            payload,
        })
        .encode(&mut self.input)
        {
            self.terminate();
            return Err(error);
        }
        let frame = match self.responses.recv_timeout(self.deadline + RESPONSE_GRACE) {
            Ok(Ok(frame)) => frame,
            Ok(Err(error)) => {
                self.terminate();
                return Err(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.terminate();
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_PROCESS_TIMEOUT: component response exceeded deadline"
                        .into(),
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.terminate();
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_PROCESS_EXIT: component response channel closed".into(),
                ));
            }
        };
        if frame.sequence != sequence || frame.kind != kind | 0x8000 {
            self.terminate();
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_PROTOCOL: response kind or sequence mismatch".into(),
            ));
        }
        let response: UiComponentResponse = postcard::from_bytes(&frame.payload)
            .map_err(|error| UiComponentError::Codec(error.to_string()))?;
        response.validate()?;
        if matches!(response, UiComponentResponse::Failed { .. }) {
            self.terminate();
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_PROCESS_FAILED: component rejected the request".into(),
            ));
        }
        if matches!(request, UiComponentRequest::Shutdown) {
            self.terminated = true;
            let status = self.child.wait().map_err(UiComponentError::Io)?;
            if !status.success() {
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_PROCESS_EXIT: component host shutdown failed".into(),
                ));
            }
        }
        Ok(response)
    }

    pub fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for UiComponentProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn request_kind(request: &UiComponentRequest) -> u16 {
    match request {
        UiComponentRequest::Open { .. } => 1,
        UiComponentRequest::Frame { .. } => 2,
        UiComponentRequest::Snapshot => 3,
        UiComponentRequest::Restore { .. } => 4,
        UiComponentRequest::Shutdown => 5,
    }
}

#[cfg(windows)]
fn configure_hidden_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_hidden_process(_command: &mut Command) {}

pub fn dylib_filename(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    }
}

pub fn validate_relative_artifact_path(path: &Path) -> Result<(), UiComponentError> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_ARTIFACT_PATH: artifact path must be relative and contained".into(),
        ));
    }
    Ok(())
}
