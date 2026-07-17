use std::{
    env, fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, OnceLock},
};

use astra_headless_protocol::{
    Envelope, JsonlReader, JsonlWriter, Message, HEADLESS_PROTOCOL_SCHEMA,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HeadlessTestError {
    #[error("headless test driver environment is missing: {0}")]
    MissingEnvironment(&'static str),
    #[error("headless test server failed: {0}")]
    Server(String),
}

struct Server {
    child: Child,
    input: JsonlWriter<ChildStdin>,
    output: JsonlReader<BufReader<ChildStdout>>,
    next_session: u64,
    active_sessions: usize,
    test_root: PathBuf,
}
static SERVER: OnceLock<Mutex<Option<Server>>> = OnceLock::new();

pub struct HeadlessTestContext {
    session: String,
    shutdown: bool,
}

impl HeadlessTestContext {
    pub fn start() -> Result<Self, HeadlessTestError> {
        let slot = SERVER.get_or_init(|| Mutex::new(None));
        let mut slot = slot
            .lock()
            .map_err(|_| HeadlessTestError::Server("server lock poisoned".into()))?;
        if slot.is_none() {
            *slot = Some(start_server()?);
        }
        let server = slot.as_mut().expect("initialized above");
        server.next_session += 1;
        server.active_sessions += 1;
        let session = format!("test-{}-{}", std::process::id(), server.next_session);
        let profile = env::var("ASTRA_HEADLESS_PROFILE")
            .map_err(|_| HeadlessTestError::MissingEnvironment("ASTRA_HEADLESS_PROFILE"))?;
        let artifact_root = env::var("ASTRA_HEADLESS_ARTIFACT_ROOT")
            .map_err(|_| HeadlessTestError::MissingEnvironment("ASTRA_HEADLESS_ARTIFACT_ROOT"))?;
        let envelope = Envelope {
            schema: HEADLESS_PROTOCOL_SCHEMA.into(),
            session: session.clone(),
            sequence: 1,
            tick: 0,
            message: Message::Open {
                profile_path: profile,
                package_path: None,
                checkpoint_config_path: None,
                artifact_root,
            },
        };
        server
            .input
            .write(&envelope)
            .map_err(|e| HeadlessTestError::Server(e.to_string()))?;
        let response: Envelope = server
            .output
            .read()
            .map_err(|e| HeadlessTestError::Server(e.to_string()))?
            .ok_or_else(|| HeadlessTestError::Server("server closed during open".into()))?;
        if !matches!(response.message, Message::Opened { .. }) {
            return Err(HeadlessTestError::Server(
                "server did not acknowledge session".into(),
            ));
        }
        Ok(Self {
            session,
            shutdown: false,
        })
    }

    pub async fn start_async() -> Result<Self, HeadlessTestError> {
        Self::start()
    }

    fn shutdown(&mut self) -> Result<(), HeadlessTestError> {
        if self.shutdown {
            return Ok(());
        }
        let slot = SERVER
            .get()
            .ok_or_else(|| HeadlessTestError::Server("server is unavailable".into()))?;
        let mut slot = slot
            .lock()
            .map_err(|_| HeadlessTestError::Server("server lock poisoned".into()))?;
        let should_stop = {
            let server = slot
                .as_mut()
                .ok_or_else(|| HeadlessTestError::Server("server is unavailable".into()))?;
            let envelope = Envelope {
                schema: HEADLESS_PROTOCOL_SCHEMA.into(),
                session: self.session.clone(),
                sequence: 2,
                tick: 0,
                message: Message::Shutdown,
            };
            server
                .input
                .write(&envelope)
                .map_err(|e| HeadlessTestError::Server(e.to_string()))?;
            let response: Envelope = server
                .output
                .read()
                .map_err(|e| HeadlessTestError::Server(e.to_string()))?
                .ok_or_else(|| HeadlessTestError::Server("server closed during shutdown".into()))?;
            if !matches!(response.message, Message::ShutdownComplete { .. }) {
                return Err(HeadlessTestError::Server(
                    "server did not complete shutdown".into(),
                ));
            }
            server.active_sessions = server
                .active_sessions
                .checked_sub(1)
                .ok_or_else(|| HeadlessTestError::Server("session count underflow".into()))?;
            server.active_sessions == 0
        };
        if should_stop {
            slot.take()
                .expect("server exists while the final session is active")
                .stop()?;
        }
        self.shutdown = true;
        Ok(())
    }
}

impl Drop for HeadlessTestContext {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            panic!("{error}");
        }
    }
}

fn start_server() -> Result<Server, HeadlessTestError> {
    let binary = headless_binary()?;
    let binary_bytes = std::fs::read(&binary).map_err(|error| {
        HeadlessTestError::Server(format!("driver binary read failed: {error}"))
    })?;
    let actual_binary_hash = format!("sha256:{:x}", Sha256::digest(&binary_bytes));
    let test_root = prepare_test_environment(&binary, &actual_binary_hash)?;
    let identity = test_root.join("build-identity.json");
    let mut command = Command::new(&binary);
    command
        .args(["serve", "--stdio", "--build-identity"])
        .arg(&identity);
    if env::var("ASTRA_HEADLESS_GPU").as_deref() == Ok("1") {
        command.arg("--gpu");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| HeadlessTestError::Server(e.to_string()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| HeadlessTestError::Server("server stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| HeadlessTestError::Server("server stdout unavailable".into()))?;
    Ok(Server {
        child,
        input: JsonlWriter::new(stdin),
        output: JsonlReader::new(BufReader::new(stdout), 1024 * 1024)
            .map_err(|e| HeadlessTestError::Server(e.to_string()))?,
        next_session: 0,
        active_sessions: 0,
        test_root,
    })
}

impl Server {
    fn stop(mut self) -> Result<(), HeadlessTestError> {
        if self
            .child
            .try_wait()
            .map_err(|error| HeadlessTestError::Server(format!("server status failed: {error}")))?
            .is_none()
        {
            self.child.kill().map_err(|error| {
                HeadlessTestError::Server(format!("server stop failed: {error}"))
            })?;
        }
        self.child
            .wait()
            .map_err(|error| HeadlessTestError::Server(format!("server wait failed: {error}")))?;
        fs::remove_dir_all(&self.test_root).map_err(|error| {
            HeadlessTestError::Server(format!("test artifact cleanup failed: {error}"))
        })?;
        Ok(())
    }
}

fn headless_binary() -> Result<PathBuf, HeadlessTestError> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_astra-headless") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    let current = env::current_exe()
        .map_err(|error| HeadlessTestError::Server(format!("test binary path failed: {error}")))?;
    let profile_root = current
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| HeadlessTestError::Server("Cargo profile root is unavailable".into()))?;
    let binary = profile_root.join(format!("astra-headless{}", env::consts::EXE_SUFFIX));
    if !binary.is_file() {
        return Err(HeadlessTestError::Server(
            "astra-headless binary is missing; run `cargo build -p astra-headless` before the test"
                .into(),
        ));
    }
    Ok(binary)
}

fn prepare_test_environment(
    binary: &Path,
    binary_hash: &str,
) -> Result<PathBuf, HeadlessTestError> {
    let current = env::current_exe()
        .map_err(|error| HeadlessTestError::Server(format!("test binary path failed: {error}")))?;
    let target_root = current
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| HeadlessTestError::Server("Cargo target root is unavailable".into()))?;
    let test_root = target_root
        .join("headless-test")
        .join(std::process::id().to_string());
    if test_root.exists() {
        fs::remove_dir_all(&test_root).map_err(|error| {
            HeadlessTestError::Server(format!("stale test artifact cleanup failed: {error}"))
        })?;
    }
    fs::create_dir_all(&test_root).map_err(|error| {
        HeadlessTestError::Server(format!("test environment creation failed: {error}"))
    })?;
    let identity = test_root.join("build-identity.json");
    let identity_payload = serde_json::json!({
        "schema": "astra.build_identity.v1",
        "identity_hash": binary_hash,
    });
    fs::write(
        &identity,
        serde_json::to_vec_pretty(&identity_payload)
            .map_err(|error| HeadlessTestError::Server(error.to_string()))?,
    )
    .map_err(|error| HeadlessTestError::Server(format!("build identity write failed: {error}")))?;
    let output = Command::new(binary)
        .args(["bootstrap-test-env", "--output"])
        .arg(&test_root)
        .arg("--build-identity")
        .arg(&identity)
        .output()
        .map_err(|error| HeadlessTestError::Server(format!("test bootstrap failed: {error}")))?;
    if !output.status.success() {
        return Err(HeadlessTestError::Server(format!(
            "test bootstrap failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if env::var("ASTRA_HEADLESS_GPU").as_deref() == Ok("1") {
        let profile_path = test_root.join("headless-profile.json");
        let profile_bytes = fs::read(&profile_path).map_err(|error| {
            HeadlessTestError::Server(format!("headless profile read failed: {error}"))
        })?;
        let mut profile: serde_json::Value =
            serde_json::from_slice(&profile_bytes).map_err(|error| {
                HeadlessTestError::Server(format!("headless profile invalid: {error}"))
            })?;
        profile["providers"]["renderer"] = serde_json::Value::String("wgpu_offscreen".into());
        fs::write(
            &profile_path,
            serde_json::to_vec_pretty(&profile)
                .map_err(|error| HeadlessTestError::Server(error.to_string()))?,
        )
        .map_err(|error| {
            HeadlessTestError::Server(format!("headless profile write failed: {error}"))
        })?;
    }
    env::set_var("ASTRA_HEADLESS_BINARY", binary);
    env::set_var("ASTRA_HEADLESS_BINARY_HASH", binary_hash);
    env::set_var(
        "ASTRA_HEADLESS_PROFILE",
        test_root.join("headless-profile.json"),
    );
    env::set_var("ASTRA_HEADLESS_PACKAGE", test_root.join("empty.astrapkg"));
    env::set_var("ASTRA_HEADLESS_ARTIFACT_ROOT", test_root.join("artifacts"));
    env::set_var("ASTRA_BUILD_IDENTITY", identity);
    Ok(test_root)
}
