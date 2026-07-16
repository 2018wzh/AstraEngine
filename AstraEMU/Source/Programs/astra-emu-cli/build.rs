use std::{env, process::Command};

use sha2::{Digest, Sha256};

fn main() {
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
    let features = "rfvp=657747252eb0d2c5fb4a340695ce6906c2d45133;features=none";
    println!(
        "cargo:rustc-env=ASTRA_EMU_FVP_FEATURE_FINGERPRINT=sha256.{}",
        hex_sha256(features.as_bytes())
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

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
