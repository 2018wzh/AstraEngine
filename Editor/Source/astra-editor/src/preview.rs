use std::{
    collections::VecDeque,
    fs::File,
    path::PathBuf,
    process::{Child, Command, Stdio},
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
}

struct Step {
    program: PathBuf,
    args: Vec<std::ffi::OsString>,
    label: &'static str,
}

pub struct Preview {
    version: u64,
    child: Option<Child>,
    steps: VecDeque<Step>,
    temporary: tempfile::TempDir,
    pub status: String,
}

impl Preview {
    pub fn start(config: &PreviewConfig, version: u64) -> anyhow::Result<Self> {
        let temporary = tempfile::tempdir()?;
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
                label: "Preview running",
                args: Vec::new(),
            },
        ]);
        let mut preview = Self {
            version,
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
            command
                .args(step.args)
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log);
            // Suppress console windows while retaining the Player's GPU window.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            self.child = Some(command.spawn()?);
            self.status = format!("{} · source v{}", step.label, self.version);
        } else {
            self.status = "Preview closed".into();
        }
        Ok(())
    }

    pub fn poll(&mut self, current_version: u64) -> anyhow::Result<()> {
        if current_version != self.version {
            self.cancel()?;
            self.status = "Preview cancelled: source changed".into();
            return Ok(());
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if let Some(status) = child.try_wait()? {
            self.child = None;
            if !status.success() {
                self.steps.clear();
                let log = std::fs::read_to_string(self.temporary.path().join("process.log"))?;
                let tail = log
                    .lines()
                    .rev()
                    .take(12)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n");
                self.status = format!("Preview failed ({status}):\n{tail}");
            } else {
                self.next()?;
            }
        }
        Ok(())
    }

    pub fn cancel(&mut self) -> anyhow::Result<()> {
        self.steps.clear();
        if let Some(mut child) = self.child.take() {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        self.status = "Preview cancelled".into();
        Ok(())
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}
