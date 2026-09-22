use std::{
    collections::VecDeque,
    fs::File,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use crate::preview_pipe::PreviewPipe;
use astra_vn_editor::{
    AuthoringWorkspace, PreviewCommand, PreviewIdentity, PreviewRequest, PreviewResponse,
    PreviewStatus, PREVIEW_PROTOCOL,
};
use serde::Deserialize;

/// Explicit tools and product bindings; paths are local user configuration.
#[derive(Clone, Deserialize)]
pub struct PreviewConfig {
    pub project: PathBuf,
    pub cli: PathBuf,
    pub player: PathBuf,
    pub profile: String,
    pub target: String,
    pub windows_runtime: Option<PathBuf>,
    pub crash_reporter: Option<PathBuf>,
}

struct Step {
    program: PathBuf,
    args: Vec<std::ffi::OsString>,
    label: &'static str,
}

pub struct Preview {
    identity: PreviewIdentity,
    sequence: u64,
    pipe: Option<PreviewPipe>,
    pub live: Option<PreviewStatus>,
    stop_deadline: Option<std::time::Instant>,
    attach_deadline: Option<std::time::Instant>,
    child: Option<Child>,
    steps: VecDeque<Step>,
    temporary: tempfile::TempDir,
    pub status: String,
}

impl Preview {
    pub fn is_finished(&self) -> bool {
        self.child.is_none() && self.steps.is_empty()
    }

    pub fn start(config: &PreviewConfig, identity: PreviewIdentity) -> anyhow::Result<Self> {
        if cfg!(windows) {
            anyhow::ensure!(config.windows_runtime.as_ref().is_some_and(|path| path.is_dir()), "Windows preview requires windows_runtime pointing to the matching VC x64 CRT directory");
            anyhow::ensure!(
                config
                    .crash_reporter
                    .as_ref()
                    .is_some_and(|path| path.is_file()),
                "Windows preview requires a built crash_reporter executable"
            );
        }
        anyhow::ensure!(
            identity.generation > 0
                && !identity.documents.is_empty()
                && identity.documents.len() <= 256
                && serde_json::to_vec(&identity)?.len() <= 32768,
            "Preview identity exceeds protocol limits"
        );
        // Cooked assets and the bundled package can be large. Keep preview
        // intermediates on the project's volume and remove this unique run on Drop.
        let project = config.project.canonicalize()?;
        let cache = project
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Project directory is missing"))?
            .join(".astra-cache")
            .join("preview");
        std::fs::create_dir_all(&cache)?;
        let temporary = tempfile::Builder::new().prefix("run-").tempdir_in(cache)?;
        let root = temporary.path();
        let cooked = root.join("cooked");
        let package = root.join("preview.astrapak");
        let bundle = root.join("bundle");
        let platform = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let player_name = if cfg!(target_os = "windows") {
            "AstraPlayer.exe"
        } else if cfg!(target_os = "macos") {
            "Contents/MacOS/astra-player"
        } else {
            "astra-player"
        };
        let mut bundle_args: Vec<std::ffi::OsString> = vec![
            "package".into(),
            "bundle".into(),
            package.clone().into(),
            "--out".into(),
            bundle.clone().into(),
            "--target".into(),
            config.target.clone().into(),
            "--profile".into(),
            config.profile.clone().into(),
            "--platform".into(),
            platform.into(),
            format!("--{platform}-player").into(),
            config.player.clone().into(),
        ];
        if let Some(runtime) = &config.windows_runtime {
            bundle_args.extend(["--windows-runtime".into(), runtime.clone().into()]);
        }
        if let Some(reporter) = &config.crash_reporter {
            bundle_args.extend(["--crash-reporter".into(), reporter.clone().into()]);
        }
        let steps = VecDeque::from([
            Step {
                program: config.cli.clone(),
                label: "Cooking",
                args: vec![
                    "cook".into(),
                    config.project.clone().into(),
                    "--profile".into(),
                    config.profile.clone().into(),
                    "--target".into(),
                    config.target.clone().into(),
                    "--out".into(),
                    cooked.clone().into(),
                ],
            },
            Step {
                program: config.cli.clone(),
                label: "Packaging",
                args: vec![
                    "package".into(),
                    "build".into(),
                    cooked.into(),
                    "--target".into(),
                    config.target.clone().into(),
                    "--out".into(),
                    package.into(),
                ],
            },
            Step {
                program: config.cli.clone(),
                label: "Bundling",
                args: bundle_args,
            },
            Step {
                program: bundle.join(player_name),
                label: "Connecting to Player",
                args: vec!["--preview-control".into()],
            },
        ]);
        let mut preview = Self {
            identity,
            sequence: 0,
            pipe: None,
            live: None,
            stop_deadline: None,
            attach_deadline: None,
            child: None,
            steps,
            temporary,
            status: String::new(),
        };
        preview.next()?;
        Ok(preview)
    }

    fn next(&mut self) -> anyhow::Result<()> {
        if let Some(step) = self.steps.pop_front() {
            let log = File::create(self.temporary.path().join("process.log"))?;
            let mut command = Command::new(step.program);
            let player = self.steps.is_empty();
            command
                .args(step.args)
                .stdin(if player {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(if player {
                    Stdio::piped()
                } else {
                    Stdio::from(log.try_clone()?)
                })
                .stderr(log);
            // Suppress console windows while retaining the Player's GPU window.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn()?;
            if player {
                self.pipe = Some(PreviewPipe::new(
                    child.stdin.take().unwrap(),
                    child.stdout.take().unwrap(),
                ));
                self.attach_deadline =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(120));
            }
            self.child = Some(child);
            self.status = step.label.into();
            if player {
                self.send(PreviewCommand::Attach)?;
            }
        } else {
            self.status = "Preview closed".into();
        }
        Ok(())
    }

    pub fn poll(&mut self, documents: &AuthoringWorkspace) -> anyhow::Result<()> {
        if documents.documents().any(|document| {
            self.identity
                .documents
                .get(&document.path)
                .is_none_or(|expected| expected.version != document.version)
        }) {
            self.cancel()?;
            self.status = "Preview cancelled: source changed".into();
            return Ok(());
        }
        while let Some(response) = self
            .pipe
            .as_ref()
            .and_then(|pipe| pipe.responses.try_recv().ok())
        {
            let response = match response {
                Ok(response) => response,
                Err(_) if self.stop_deadline.is_some() => break,
                Err(error) => anyhow::bail!("{error}\n{}", self.log_tail()),
            };
            match response {
                PreviewResponse::Ready {
                    protocol,
                    sequence,
                    status,
                } => {
                    anyhow::ensure!(
                        protocol == PREVIEW_PROTOCOL && sequence == 1 && self.live.is_none(),
                        "Unexpected Player ready response"
                    );
                    self.accept_status(status)?;
                    self.attach_deadline = None;
                }
                PreviewResponse::State { sequence, status } => {
                    if self.stop_deadline.is_some() {
                        continue;
                    }
                    anyhow::ensure!(
                        self.live.is_some() && sequence <= self.sequence,
                        "Unexpected Player state response"
                    );
                    self.accept_status(status)?;
                }
                PreviewResponse::Rejected { code, .. } => {
                    self.status = format!("Preview command rejected: {code}")
                }
                PreviewResponse::Failure { code } => anyhow::bail!("Preview failed: {code}"),
                PreviewResponse::Stopped => {
                    self.live = None;
                    self.status = "Preview stopped".into();
                    self.stop_deadline =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                    break;
                }
            }
        }
        let now = std::time::Instant::now();
        anyhow::ensure!(
            self.stop_deadline.is_some()
                || !self
                    .pipe
                    .as_ref()
                    .is_some_and(|pipe| pipe.reader_finished()),
            "Player control stream ended unexpectedly\n{}",
            self.log_tail()
        );
        if self.stop_deadline.is_some_and(|deadline| now >= deadline) {
            self.cancel()?;
            self.status = "Preview stopped (process exit timeout)".into();
            return Ok(());
        }
        anyhow::ensure!(
            !self.attach_deadline.is_some_and(|deadline| now >= deadline),
            "Player connection timed out\n{}",
            self.log_tail()
        );
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if let Some(status) = child.try_wait()? {
            self.child = None;
            if let Some(mut pipe) = self.pipe.take() {
                pipe.join();
            }
            self.live = None;
            self.stop_deadline = None;
            self.attach_deadline = None;
            if !status.success() {
                self.steps.clear();
                let tail = self.log_tail();
                self.status = format!("Preview failed ({status}):\n{tail}");
            } else {
                self.next()?;
            }
        }
        Ok(())
    }

    fn log_tail(&self) -> String {
        use std::io::{Read, Seek, SeekFrom};
        let read = || -> std::io::Result<String> {
            let mut file = File::open(self.temporary.path().join("process.log"))?;
            let length = file.metadata()?.len();
            file.seek(SeekFrom::Start(length.saturating_sub(16_384)))?;
            let mut bytes = Vec::new();
            file.take(16_384).read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"))
        };
        read().unwrap_or_else(|error| format!("Could not read preview diagnostics: {error}"))
    }

    pub fn cancel(&mut self) -> anyhow::Result<()> {
        self.steps.clear();
        if let Some(child) = self.child.as_mut() {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        self.child = None;
        if let Some(mut pipe) = self.pipe.take() {
            pipe.join();
        }
        self.live = None;
        self.stop_deadline = None;
        self.attach_deadline = None;
        self.status = "Preview cancelled".into();
        Ok(())
    }

    fn accept_status(&mut self, status: PreviewStatus) -> anyhow::Result<()> {
        anyhow::ensure!(
            status.identity == self.identity,
            "Player returned a stale preview identity"
        );
        self.status = format!(
            "{} · {:.3}s · {}",
            if status.paused { "Paused" } else { "Playing" },
            status.presentation_time_ns as f64 / 1_000_000_000.,
            status
                .source_id
                .as_deref()
                .unwrap_or("no seekable fragment")
        );
        self.live = Some(status);
        Ok(())
    }

    pub fn send(&mut self, command: PreviewCommand) -> anyhow::Result<()> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Preview sequence exhausted"))?;
        self.pipe
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Player is not connected"))?
            .send(&PreviewRequest {
                protocol: PREVIEW_PROTOCOL.into(),
                sequence: self.sequence,
                identity: self.identity.clone(),
                command,
            })
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        if self.live.is_some() {
            self.send(PreviewCommand::Stop)?;
            self.live = None;
            self.status = "Stopping Player".into();
            self.stop_deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            Ok(())
        } else {
            self.cancel()
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_core::Hash256;
    use astra_vn_editor::{AstraSource, PreviewDocumentRevision};
    use std::collections::BTreeMap;

    fn idle_preview() -> Preview {
        Preview {
            identity: PreviewIdentity {
                project_hash: Hash256::from_sha256(b"project"),
                documents: BTreeMap::from([(
                    "main.astra".into(),
                    PreviewDocumentRevision {
                        version: 1,
                        content_hash: Hash256::from_sha256(b"# source"),
                    },
                )]),
                generation: 1,
            },
            sequence: 0,
            pipe: None,
            live: None,
            stop_deadline: None,
            attach_deadline: None,
            child: None,
            steps: VecDeque::new(),
            temporary: tempfile::tempdir().unwrap(),
            status: String::new(),
        }
    }

    #[test]
    fn player_status_requires_exact_identity_and_reports_actual_position() {
        let mut preview = idle_preview();
        let mut status = PreviewStatus {
            identity: preview.identity.clone(),
            paused: true,
            presentation_time_ns: 250_000_000,
            source_id: Some("line.1".into()),
            checkpoints: vec![astra_vn_editor::PreviewCheckpointInfo {
                id: 7,
                presentation_time_ns: 900_000_000,
            }],
        };
        preview.accept_status(status.clone()).unwrap();
        assert!(preview.status.contains("0.250s"));
        status.identity.generation += 1;
        assert!(preview.accept_status(status).is_err());
        assert_eq!(preview.live.as_ref().unwrap().identity.generation, 1);
    }

    #[test]
    fn changes_in_any_document_invalidate_the_preview() {
        let mut preview = idle_preview();
        let mut documents = AuthoringWorkspace::default();
        documents
            .open(AstraSource::story("main.astra", "# source"))
            .unwrap();
        documents
            .open(AstraSource::story("second.astra", "# second"))
            .unwrap();
        preview.poll(&documents).unwrap();
        assert_eq!(preview.status, "Preview cancelled: source changed");
    }
}
