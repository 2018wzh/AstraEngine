use std::{env, fs, process::Command};

use astra_emu_family_api::LEGACY_FAMILY_ABI_FINGERPRINT;
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() {
    let rustc = env::var_os("RUSTC").expect("ASTRA_MINORI_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_MINORI_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_MINORI_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_MINORI_BUILD_RUSTC_IDENTITY_NOT_UTF8")
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
        "ASTRA_MINORI_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    let rustc_fingerprint = format!("sha256.{}", hex_sha256(identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_MINORI_RUSTC_FINGERPRINT={rustc_fingerprint}");

    let mut features = env::vars()
        .filter_map(|(name, _)| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .filter(|name| !matches!(name.as_str(), "DEFAULT" | "DYNAMIC_PLUGIN_EXPORT"))
        .collect::<Vec<_>>();
    features.sort();
    let feature_identity = format!(
        "garbro=b09ee4570ccb1daf6ac56710ee8934dc0b8baeb0;features={}",
        if features.is_empty() {
            "none".into()
        } else {
            features.join(",").to_ascii_lowercase()
        }
    );
    let feature_fingerprint = format!("sha256.{}", hex_sha256(feature_identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_MINORI_FEATURE_FINGERPRINT={feature_fingerprint}");

    let descriptor = json!({
        "family_id": "minori",
        "plugin_id": "astra.emu.minori",
        "provider_id": "astra.emu.family.minori",
        "engine_version": env::var("CARGO_PKG_VERSION").expect("ASTRA_MINORI_VERSION_MISSING"),
        "rustc_fingerprint": rustc_fingerprint,
        "feature_fingerprint": feature_fingerprint,
        "abi_fingerprint": LEGACY_FAMILY_ABI_FINGERPRINT,
        "supported_formats": ["minori.sc", "minori.paz", "minori.ani", "minori.sqz"],
        "permissions": ["vfs.read", "media.submit", "storage.request"],
        "report_redaction": "astra.emu.redaction.v1",
        "license": "MPL-2.0"
    });
    let out_dir = env::var_os("OUT_DIR").expect("ASTRA_MINORI_OUT_DIR_MISSING");
    fs::write(
        std::path::Path::new(&out_dir).join("astra-minori-descriptor.json"),
        serde_json::to_vec_pretty(&descriptor).expect("ASTRA_MINORI_DESCRIPTOR_SERIALIZE"),
    )
    .expect("ASTRA_MINORI_DESCRIPTOR_WRITE");
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
