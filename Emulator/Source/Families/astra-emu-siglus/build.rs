use std::{env, fs, path::Path, process::Command};

use astra_emu_family_api::LEGACY_FAMILY_ABI_FINGERPRINT;
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() {
    let rustc = env::var_os("RUSTC").expect("ASTRA_SIGLUS_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_SIGLUS_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_SIGLUS_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_SIGLUS_BUILD_RUSTC_IDENTITY_NOT_UTF8")
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
        "ASTRA_SIGLUS_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    let rustc_fingerprint = format!("sha256.{}", hex_sha256(identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_SIGLUS_RUSTC_FINGERPRINT={rustc_fingerprint}");

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let hosted_revision = hosted_fork_revision(&manifest_path);
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let feature_fingerprint = format!(
        "sha256.{}",
        hex_sha256(format!("siglus={hosted_revision};features=hosted").as_bytes())
    );
    println!("cargo:rustc-env=ASTRA_SIGLUS_FEATURE_FINGERPRINT={feature_fingerprint}");

    let descriptor = json!({
        "family_id": "siglus",
        "plugin_id": "astra.emu.siglus",
        "provider_id": "astra.emu.family.siglus",
        "engine_version": env::var("CARGO_PKG_VERSION").expect("ASTRA_SIGLUS_VERSION_MISSING"),
        "rustc_fingerprint": rustc_fingerprint,
        "feature_fingerprint": feature_fingerprint,
        "abi_fingerprint": LEGACY_FAMILY_ABI_FINGERPRINT,
        "supported_formats": ["siglus.gameexe", "siglus.scene_pck", "siglus.g00", "siglus.nwa", "siglus.ovk", "siglus.omv"],
        "permissions": ["vfs.read", "media.submit", "text.layout", "private_material.read", "save.atomic_write"],
        "report_redaction": "astra.emu.redaction.v1",
        "license": "MPL-2.0"
    });
    let out_dir = env::var_os("OUT_DIR").expect("ASTRA_SIGLUS_OUT_DIR_MISSING");
    fs::write(
        Path::new(&out_dir).join("astra-siglus-descriptor.json"),
        serde_json::to_vec_pretty(&descriptor).expect("ASTRA_SIGLUS_DESCRIPTOR_SERIALIZE"),
    )
    .expect("ASTRA_SIGLUS_DESCRIPTOR_WRITE");
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hosted_fork_revision(manifest_path: &Path) -> String {
    let manifest = fs::read_to_string(manifest_path).expect("ASTRA_SIGLUS_MANIFEST_READ_FAILED");
    let prefix = "hosted_fork_revision = \"";
    let revision = manifest
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(prefix))
        .and_then(|value| value.strip_suffix('"'))
        .expect("ASTRA_SIGLUS_HOSTED_FORK_REVISION_MISSING");
    assert!(
        revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "ASTRA_SIGLUS_HOSTED_FORK_REVISION_INVALID"
    );
    revision.to_owned()
}
