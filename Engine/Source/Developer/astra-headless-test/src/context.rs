use std::{
    env,
    io::BufReader,
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
    _child: Child,
    input: JsonlWriter<ChildStdin>,
    output: JsonlReader<BufReader<ChildStdout>>,
    next_session: u64,
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
    let binary = env::var("ASTRA_HEADLESS_BINARY")
        .map_err(|_| HeadlessTestError::MissingEnvironment("ASTRA_HEADLESS_BINARY"))?;
    let expected_binary_hash = env::var("ASTRA_HEADLESS_BINARY_HASH")
        .map_err(|_| HeadlessTestError::MissingEnvironment("ASTRA_HEADLESS_BINARY_HASH"))?;
    let binary_bytes = std::fs::read(&binary).map_err(|error| {
        HeadlessTestError::Server(format!("driver binary read failed: {error}"))
    })?;
    let actual_binary_hash = format!("sha256:{:x}", Sha256::digest(&binary_bytes));
    if actual_binary_hash != expected_binary_hash {
        return Err(HeadlessTestError::Server(
            "driver binary hash does not match checkout-bound identity".into(),
        ));
    }
    let identity = env::var("ASTRA_BUILD_IDENTITY")
        .map_err(|_| HeadlessTestError::MissingEnvironment("ASTRA_BUILD_IDENTITY"))?;
    let mut child = Command::new(binary)
        .args(["serve", "--stdio", "--build-identity", &identity])
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
        _child: child,
        input: JsonlWriter::new(stdin),
        output: JsonlReader::new(BufReader::new(stdout), 1024 * 1024)
            .map_err(|e| HeadlessTestError::Server(e.to_string()))?,
        next_session: 0,
    })
}
