use std::{env, fs, path::PathBuf, process::Command};

use sha2::{Digest, Sha256};

fn main() {
    let rustc = env::var_os("RUSTC").expect("ASTRA_EMU_MANAGER_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_EMU_MANAGER_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_EMU_MANAGER_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_EMU_MANAGER_BUILD_RUSTC_IDENTITY_NOT_UTF8")
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
        "ASTRA_EMU_MANAGER_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    println!(
        "cargo:rustc-env=ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT=sha256.{}",
        hex_sha256(identity.as_bytes())
    );
    let features = "rfvp=3b5ea6c96a925c12f95aef8554905e8fecbc77c3;features=none";
    println!(
        "cargo:rustc-env=ASTRA_EMU_FVP_FEATURE_FINGERPRINT=sha256.{}",
        hex_sha256(features.as_bytes())
    );
    println!(
        "cargo:rustc-env=ASTRA_EMU_TARGET={}",
        env::var("TARGET").expect("ASTRA_EMU_MANAGER_BUILD_TARGET_MISSING")
    );
    configure_static_ios_registration();
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

fn configure_static_ios_registration() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("ASTRA_EMU_OUT_DIR_MISSING"))
        .join("astra-emu-fvp-static-manifest.json");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        fs::write(output, b"{}").expect("ASTRA_EMU_STATIC_MANIFEST_STUB_WRITE_FAILED");
        return;
    }
    let manifest_path =
        env::var("ASTRA_EMU_FVP_STATIC_MANIFEST").expect("ASTRA_EMU_FVP_STATIC_MANIFEST_MISSING");
    let archive_path =
        env::var("ASTRA_EMU_FVP_STATIC_ARCHIVE").expect("ASTRA_EMU_FVP_STATIC_ARCHIVE_MISSING");
    let manifest = fs::read(&manifest_path).expect("ASTRA_EMU_FVP_STATIC_MANIFEST_READ_FAILED");
    let archive = fs::read(&archive_path).expect("ASTRA_EMU_FVP_STATIC_ARCHIVE_READ_FAILED");
    assert!(
        !manifest.is_empty() && manifest.len() <= 1024 * 1024,
        "ASTRA_EMU_FVP_STATIC_MANIFEST_BOUNDS"
    );
    assert!(!archive.is_empty(), "ASTRA_EMU_FVP_STATIC_ARCHIVE_EMPTY");
    fs::write(output, manifest).expect("ASTRA_EMU_FVP_STATIC_MANIFEST_COPY_FAILED");
    println!(
        "cargo:rustc-env=ASTRA_EMU_FVP_STATIC_BINARY_HASH=sha256:{}",
        hex_sha256(&archive)
    );
    println!("cargo:rerun-if-changed={manifest_path}");
    println!("cargo:rerun-if-changed={archive_path}");
    println!("cargo:rerun-if-env-changed=ASTRA_EMU_FVP_STATIC_MANIFEST");
    println!("cargo:rerun-if-env-changed=ASTRA_EMU_FVP_STATIC_ARCHIVE");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
