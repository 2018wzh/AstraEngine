use std::{env, fs, path::Path, process::Command};

use sha2::{Digest, Sha256};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("ASTRA_EMU_CLI_MANIFEST_DIR_MISSING");
    let source_root = git_output(&manifest_dir, ["rev-parse", "--show-toplevel"]);
    let source_revision = git_output(&source_root, ["rev-parse", "HEAD"]);
    assert!(
        source_revision.len() == 40 && source_revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "ASTRA_EMU_CLI_SOURCE_REVISION_INVALID"
    );
    let source_dirty = !git_output(
        &source_root,
        ["status", "--porcelain", "--untracked-files=no"],
    )
    .is_empty();
    for git_path in [
        git_output(&source_root, ["rev-parse", "--git-path", "HEAD"]),
        git_output(&source_root, ["rev-parse", "--git-path", "index"]),
    ] {
        println!("cargo:rerun-if-changed={git_path}");
    }
    println!("cargo:rustc-env=ASTRA_EMU_CLI_SOURCE_REVISION={source_revision}");
    println!(
        "cargo:rustc-env=ASTRA_EMU_CLI_SOURCE_DIRTY={}",
        u8::from(source_dirty)
    );
    let rustc = env::var_os("RUSTC").expect("ASTRA_EMU_CLI_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_EMU_CLI_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_EMU_CLI_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_EMU_CLI_BUILD_RUSTC_IDENTITY_NOT_UTF8")
        .lines()
        .filter(|line| {
            line.starts_with("release:")
                || line.starts_with("commit-hash:")
                || line.starts_with("host:")
                || line.starts_with("LLVM version:")
        })
        .collect::<Vec<_>>()
        .join(";");
    assert!(
        !identity.is_empty(),
        "ASTRA_EMU_CLI_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    println!(
        "cargo:rustc-env=ASTRA_EMU_CLI_RUSTC_FINGERPRINT=sha256.{}",
        hex_sha256(identity.as_bytes())
    );
    let fvp_manifest = Path::new(&manifest_dir).join("../../Families/astra-emu-fvp/Cargo.toml");
    let hosted_fork_revision = hosted_fork_revision(&fvp_manifest);
    println!("cargo:rerun-if-changed={}", fvp_manifest.display());
    let features = format!("rfvp={hosted_fork_revision};features=none");
    println!(
        "cargo:rustc-env=ASTRA_EMU_FVP_FEATURE_FINGERPRINT=sha256.{}",
        hex_sha256(features.as_bytes())
    );
    let minori_features = "garbro=b09ee4570ccb1daf6ac56710ee8934dc0b8baeb0;features=none";
    println!(
        "cargo:rustc-env=ASTRA_EMU_MINORI_FEATURE_FINGERPRINT=sha256.{}",
        hex_sha256(minori_features.as_bytes())
    );
    println!(
        "cargo:rustc-env=ASTRA_EMU_TARGET={}",
        env::var("TARGET").expect("ASTRA_EMU_CLI_BUILD_TARGET_MISSING")
    );
    for name in [
        "ASTRA_EMU_FAMILY_SIGNER_ID",
        "ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        println!(
            "cargo:rustc-env={name}={}",
            env::var(name).unwrap_or_default()
        );
    }
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn git_output<const N: usize>(cwd: &str, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(arguments)
        .output()
        .expect("ASTRA_EMU_CLI_GIT_EXECUTION_FAILED");
    assert!(output.status.success(), "ASTRA_EMU_CLI_GIT_QUERY_FAILED");
    String::from_utf8(output.stdout)
        .expect("ASTRA_EMU_CLI_GIT_OUTPUT_NOT_UTF8")
        .trim()
        .to_owned()
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hosted_fork_revision(manifest_path: &Path) -> String {
    let manifest =
        fs::read_to_string(manifest_path).expect("ASTRA_EMU_CLI_FVP_MANIFEST_READ_FAILED");
    let prefix = "hosted_fork_revision = \"";
    let revision = manifest
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(prefix))
        .and_then(|value| value.strip_suffix('"'))
        .expect("ASTRA_EMU_CLI_HOSTED_FORK_REVISION_MISSING");
    assert!(
        revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "ASTRA_EMU_CLI_HOSTED_FORK_REVISION_INVALID"
    );
    revision.to_owned()
}
