use std::{
    ffi::OsString,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use wait_timeout::ChildExt;

use crate::{
    error::{PatchError, PatchResult},
    filesystem::hash_file,
};

const PROJECTORRAYS_ID: &str = "ProjectorRays";
const PROJECTORRAYS_VERSION: &str = "0.2.0";
const PROJECTORRAYS_SHA256: &str =
    "e9814428ee503cf129b6f5cff54524177b7bdd63201a9095d8d19433535c70db";
const PROJECTORRAYS_FILENAME: &str = "projectorrays-0.2.0.exe";
const HELPER_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperIdentity {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

impl HelperIdentity {
    pub fn from_validated(value: ValidatedHelper) -> Self {
        Self {
            id: value.id,
            version: value.version,
            sha256: value.sha256,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedHelper {
    id: String,
    version: String,
    sha256: String,
}

pub trait HelperPolicy {
    fn validate(&self, helper: &Path) -> PatchResult<ValidatedHelper>;
}

pub struct ReleaseHelperPolicy;

impl HelperPolicy for ReleaseHelperPolicy {
    fn validate(&self, helper: &Path) -> PatchResult<ValidatedHelper> {
        let metadata = fs::symlink_metadata(helper).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_HELPER_MISSING",
                "inspect bundled ProjectorRays helper",
                error,
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PatchError::helper(
                "TSUI_PATCH_HELPER_TYPE_INVALID",
                "bundled ProjectorRays helper must be a regular file",
            ));
        }
        let sha256 = hash_file(helper)?;
        if sha256 != PROJECTORRAYS_SHA256 {
            return Err(PatchError::helper(
                "TSUI_PATCH_HELPER_HASH_MISMATCH",
                "bundled ProjectorRays helper hash is not approved",
            ));
        }
        Ok(ValidatedHelper {
            id: PROJECTORRAYS_ID.to_owned(),
            version: PROJECTORRAYS_VERSION.to_owned(),
            sha256,
        })
    }
}

pub fn resolve_helper(explicit: Option<&Path>) -> PatchResult<PathBuf> {
    let path = match explicit {
        Some(path) => path.to_owned(),
        None => std::env::current_exe()
            .map_err(|error| {
                PatchError::io(
                    "TSUI_PATCH_EXECUTABLE_PATH_UNAVAILABLE",
                    "resolve patcher executable",
                    error,
                )
            })?
            .parent()
            .ok_or_else(|| {
                PatchError::helper(
                    "TSUI_PATCH_HELPER_LOCATION_INVALID",
                    "patcher executable has no parent directory",
                )
            })?
            .join(PROJECTORRAYS_FILENAME),
    };
    Ok(path)
}

pub fn decompile_menu(helper: &Path, input: &Path, output: &Path) -> PatchResult<()> {
    run_decompile(
        dunce::simplified(helper),
        &[],
        dunce::simplified(input),
        dunce::simplified(output),
        HELPER_TIMEOUT,
    )
}

fn run_decompile(
    program: &Path,
    prefix_args: &[OsString],
    input: &Path,
    output: &Path,
    timeout: Duration,
) -> PatchResult<()> {
    let parent = output.parent().ok_or_else(|| {
        PatchError::helper(
            "TSUI_PATCH_HELPER_OUTPUT_INVALID",
            "ProjectorRays output has no parent directory",
        )
    })?;
    let stdout_path = parent.join(".projectorrays.stdout");
    let stderr_path = parent.join(".projectorrays.stderr");
    let stdout = File::create(&stdout_path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_HELPER_LOG_CREATE_FAILED",
            "create bounded ProjectorRays stdout capture",
            error,
        )
    })?;
    let stderr = File::create(&stderr_path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_HELPER_LOG_CREATE_FAILED",
            "create bounded ProjectorRays stderr capture",
            error,
        )
    })?;
    let mut command = Command::new(program);
    command
        .args(prefix_args)
        .arg("decompile")
        .arg(input)
        .arg("-o")
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().map_err(|error| {
        PatchError::helper(
            "TSUI_PATCH_HELPER_START_FAILED",
            format!("ProjectorRays could not start: {error}"),
        )
    })?;
    let status = child.wait_timeout(timeout).map_err(|error| {
        PatchError::helper(
            "TSUI_PATCH_HELPER_WAIT_FAILED",
            format!("ProjectorRays wait failed: {error}"),
        )
    })?;
    let status = match status {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            cleanup_logs(&stdout_path, &stderr_path);
            return Err(PatchError::helper(
                "TSUI_PATCH_HELPER_TIMEOUT",
                "ProjectorRays exceeded the 120 second time limit",
            ));
        }
    };
    if !status.success() {
        let stdout_hash = hash_file(&stdout_path).unwrap_or_else(|_| "unavailable".to_owned());
        let stderr_hash = hash_file(&stderr_path).unwrap_or_else(|_| "unavailable".to_owned());
        let stderr_summary = read_redacted_summary(
            &stderr_path,
            &[program, input, output, output.parent().unwrap_or(output)],
        );
        cleanup_logs(&stdout_path, &stderr_path);
        return Err(PatchError::helper(
            "TSUI_PATCH_HELPER_FAILED",
            format!(
                "ProjectorRays failed (exit_code={}, stdout_sha256={}, stderr_sha256={}, stderr={})",
                status.code().unwrap_or(-1),
                stdout_hash,
                stderr_hash,
                stderr_summary
            ),
        ));
    }
    cleanup_logs(&stdout_path, &stderr_path);
    let metadata = fs::metadata(output).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_HELPER_OUTPUT_MISSING",
            "inspect ProjectorRays output",
            error,
        )
    })?;
    if !metadata.is_file() || metadata.len() < 32 {
        return Err(PatchError::helper(
            "TSUI_PATCH_HELPER_OUTPUT_INVALID",
            "ProjectorRays did not produce a valid movie file",
        ));
    }
    Ok(())
}

fn cleanup_logs(stdout: &Path, stderr: &Path) {
    let _ = fs::remove_file(stdout);
    let _ = fs::remove_file(stderr);
}

fn read_redacted_summary(path: &Path, sensitive_paths: &[&Path]) -> String {
    const LIMIT: u64 = 8 * 1024;
    let mut bytes = Vec::new();
    let result = File::open(path).and_then(|file| file.take(LIMIT).read_to_end(&mut bytes));
    if result.is_err() {
        return "unavailable".to_owned();
    }
    let mut text = String::from_utf8_lossy(&bytes).replace(['\r', '\n'], " ");
    for (index, sensitive) in sensitive_paths.iter().enumerate() {
        if let Some(value) = sensitive.to_str() {
            text = text.replace(value, &format!("<path-{index}>"));
            text = text.replace(&value.replace('\\', "/"), &format!("<path-{index}>"));
        }
    }
    let text = text.trim();
    if text.is_empty() {
        "empty".to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn rejects_unapproved_helper_hash() {
        let temp = tempdir().expect("tempdir");
        let helper = temp.path().join("projectorrays.exe");
        fs::write(&helper, b"not ProjectorRays").expect("helper");
        let error = ReleaseHelperPolicy
            .validate(&helper)
            .expect_err("hash mismatch must fail");
        assert_eq!(error.code, "TSUI_PATCH_HELPER_HASH_MISMATCH");
    }

    #[test]
    fn fake_helper_failure_is_bounded_and_does_not_expose_output() {
        let temp = tempdir().expect("tempdir");
        let script = temp.path().join("fake_projectorrays.py");
        fs::write(
            &script,
            "import sys\nprint('private path should stay captured')\nsys.exit(7)\n",
        )
        .expect("script");
        let input = temp.path().join("input.dxr");
        let output = temp.path().join("output.dxr");
        fs::write(&input, b"input").expect("input");
        let error = run_decompile(
            Path::new("python"),
            &[script.into_os_string()],
            &input,
            &output,
            Duration::from_secs(10),
        )
        .expect_err("helper failure");
        assert_eq!(error.code, "TSUI_PATCH_HELPER_FAILED");
        assert!(!error.message.contains("private path"));
        assert!(!temp.path().join(".projectorrays.stdout").exists());
        assert!(!temp.path().join(".projectorrays.stderr").exists());
    }

    #[test]
    fn redacted_summary_removes_known_paths_and_line_breaks() {
        let temp = tempdir().expect("tempdir");
        let log = temp.path().join("stderr.log");
        let secret = temp.path().join("private").join("MENU.dxr");
        fs::write(&log, format!("failed at {}\r\nnext", secret.display())).expect("log");
        let summary = read_redacted_summary(&log, &[&secret]);
        assert_eq!(summary, "failed at <path-0>  next");
        assert!(!summary.contains(&secret.display().to_string()));
    }
}
