use std::{env, fs, process::Command};

use astra_emu_family_api::{
    FamilyId, LegacyFamilyCoreKind, LegacyFamilyPluginDescriptor, LegacyFamilyPresentationMode,
    LEGACY_FAMILY_ABI_FINGERPRINT,
};
use sha2::{Digest, Sha256};

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_FFMPEG_VCPKG");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_FEATURE");
    let rustc = env::var_os("RUSTC").expect("ASTRA_MUSICA_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("ASTRA_MUSICA_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "ASTRA_MUSICA_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("ASTRA_MUSICA_BUILD_RUSTC_IDENTITY_NOT_UTF8")
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
        "ASTRA_MUSICA_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    let rustc_fingerprint = format!("sha256.{}", hex_sha256(identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_MUSICA_RUSTC_FINGERPRINT={rustc_fingerprint}");

    // Cargo exposes activated package features to build scripts through the
    // `CARGO_FEATURE_*` variables.  `CARGO_CFG_FEATURE` is a rustc cfg value,
    // not a reliable build-script input (it is absent or empty on some Cargo
    // versions).  Keep it as a supplemental source for older toolchains, then
    // normalize and deduplicate both sources before hashing the identity so a
    // descriptor always describes the binary that was actually built.
    let mut features = env::vars()
        .filter_map(|(name, value)| {
            let feature = name.strip_prefix("CARGO_FEATURE_")?;
            (value == "1").then(|| feature.to_ascii_lowercase().replace('_', "-"))
        })
        .chain(
            env::var("CARGO_CFG_FEATURE")
                .unwrap_or_default()
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_ascii_lowercase),
        )
        .filter(|name| !matches!(name.as_str(), "default" | "dynamic-plugin-export"))
        .collect::<Vec<_>>();
    features.sort();
    features.dedup();
    let feature_identity = format!(
        "garbro=b09ee4570ccb1daf6ac56710ee8934dc0b8baeb0;features={}",
        if features.is_empty() {
            "none".into()
        } else {
            features.join(",").to_ascii_lowercase()
        }
    );
    let feature_fingerprint = format!("sha256.{}", hex_sha256(feature_identity.as_bytes()));
    println!("cargo:rustc-env=ASTRA_MUSICA_FEATURE_FINGERPRINT={feature_fingerprint}");

    let descriptor = LegacyFamilyPluginDescriptor {
        family_id: FamilyId("musica".into()),
        plugin_id: "astra.emu.musica".into(),
        provider_id: "astra.emu.family.musica".into(),
        core_kind: LegacyFamilyCoreKind::Native,
        presentation_mode: LegacyFamilyPresentationMode::MultiLayer,
        engine_version: env::var("CARGO_PKG_VERSION").expect("ASTRA_MUSICA_VERSION_MISSING"),
        rustc_fingerprint,
        feature_fingerprint,
        abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
        supported_formats: vec![
            "musica.sc".into(),
            "musica.paz".into(),
            "musica.ani".into(),
            "musica.sqz".into(),
        ],
        permissions: vec![
            "vfs.read".into(),
            "surface.write".into(),
            "hook.invoke".into(),
            "media.submit".into(),
            "writable_file".into(),
        ],
        report_redaction: "astra.emu.redaction.v1".into(),
        license: "MPL-2.0".into(),
    };
    descriptor
        .validate()
        .expect("ASTRA_MUSICA_DESCRIPTOR_INVALID");
    let out_dir = env::var_os("OUT_DIR").expect("ASTRA_MUSICA_OUT_DIR_MISSING");
    fs::write(
        std::path::Path::new(&out_dir).join("astra-musica-descriptor.json"),
        serde_json::to_vec_pretty(&descriptor).expect("ASTRA_MUSICA_DESCRIPTOR_SERIALIZE"),
    )
    .expect("ASTRA_MUSICA_DESCRIPTOR_WRITE");
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
