use std::{env, fs, process::Command};

use astra_emu_family_api::LEGACY_FAMILY_ABI_FINGERPRINT;
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() {
    let rustc = env::var_os("RUSTC").expect("ASTRA_FVP_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_FVP_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_FVP_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_FVP_BUILD_RUSTC_IDENTITY_NOT_UTF8")
        .lines()
        .filter(|line| {
            line.starts_with("release:")
                || line.starts_with("commit-hash:")
                || line.starts_with("host:")
                || line.starts_with("LLVM version:")
        })
        .collect::<Vec<_>>()
        .join(";");
    assert!(!identity.is_empty(), "ASTRA_FVP_BUILD_RUSTC_IDENTITY_EMPTY");
    let rustc_fingerprint = format!("sha256.{}", hex_sha256(identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_FVP_RUSTC_FINGERPRINT={rustc_fingerprint}");

    let mut features = env::vars()
        .filter_map(|(name, _)| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .collect::<Vec<_>>();
    features.sort();
    let feature_identity = if features.is_empty() {
        "none".to_owned()
    } else {
        features.join(",").to_ascii_lowercase()
    };
    let feature_identity =
        format!("rfvp=3b5ea6c96a925c12f95aef8554905e8fecbc77c3;features={feature_identity}");
    let feature_fingerprint = format!("sha256.{}", hex_sha256(feature_identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_FVP_FEATURE_FINGERPRINT={feature_fingerprint}");
    let descriptor = json!({
        "family_id": "fvp",
        "plugin_id": "astra.emu.fvp",
        "provider_id": "astra.emu.family.fvp",
        "engine_version": env::var("CARGO_PKG_VERSION").expect("ASTRA_FVP_VERSION_MISSING"),
        "rustc_fingerprint": rustc_fingerprint,
        "feature_fingerprint": feature_fingerprint,
        "abi_fingerprint": LEGACY_FAMILY_ABI_FINGERPRINT,
        "supported_formats": ["fvp.hcb", "fvp.bin", "fvp.nvsg", "fvp.hzc1"],
        "permissions": ["vfs.read", "media.submit"],
        "report_redaction": "astra.emu.redaction.v1",
        "license": "MPL-2.0"
    });
    let out_dir = env::var_os("OUT_DIR").expect("ASTRA_FVP_OUT_DIR_MISSING");
    fs::write(
        std::path::Path::new(&out_dir).join("astra-fvp-descriptor.json"),
        serde_json::to_vec_pretty(&descriptor).expect("ASTRA_FVP_DESCRIPTOR_SERIALIZE"),
    )
    .expect("ASTRA_FVP_DESCRIPTOR_WRITE");
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
