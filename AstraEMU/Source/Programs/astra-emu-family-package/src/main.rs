use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use astra_core::Hash256;
use astra_emu_family_api::LegacyFamilyPluginDescriptor;
use astra_emu_manager_core::{
    canonical_family_manifest_bytes, family_base_identity_hash, inspect_dynamic_family_descriptor,
    AndroidNativePluginManifest, Ed25519FamilySignatureVerifier, FamilyPluginManifest,
};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use object::{read::archive::ArchiveFile, Architecture, BinaryFormat, Object};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PLUGIN_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "astra-emu-family-package")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Derive the public trust root for a signing key without exposing the
    /// private key in process arguments or output.
    PublicKey {
        #[arg(long, default_value = "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX")]
        signing_key_env: String,
    },
    Sign {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX")]
        signing_key_env: String,
    },
    /// Inspect and sign a native desktop family library without accepting an
    /// unsigned manifest template as an additional identity authority.
    NativeSign {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        descriptor: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        signer_identity: String,
        #[arg(long, default_value = "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX")]
        signing_key_env: String,
    },
    /// Sign a statically linked family archive for the iOS registry. Unlike
    /// `sign`, this consumes the build-script descriptor because a static
    /// archive cannot expose the dynamic ABI root module to the packaging host.
    StaticSign {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        descriptor: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        signer_identity: String,
        #[arg(long, default_value = "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX")]
        signing_key_env: String,
    },
    /// Produce an atomic APK-bound FVP metadata directory containing the
    /// signed family manifest and Android native manifest.
    AndroidSign {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long, required_unless_present = "manifest")]
        descriptor: Option<PathBuf>,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        version_code: u64,
        #[arg(long)]
        abi: String,
        #[arg(long)]
        apk_signer_sha256: String,
        #[arg(long)]
        signer_identity: String,
        #[arg(long, default_value_t = 26)]
        min_api: u32,
        #[arg(long, default_value_t = 36)]
        target_api: u32,
        #[arg(long, default_value = "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX")]
        signing_key_env: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _observability = astra_observability::init_host(
        astra_observability::HostObservabilityConfig::for_cli("info"),
    )?;
    tracing::info!(event = "astra.emu.family_package.started");
    let result = run();
    match &result {
        Ok(()) => tracing::info!(event = "astra.emu.family_package.completed"),
        Err(_) => tracing::error!(
            event = "astra.emu.family_package.failed",
            diagnostic_code = "ASTRA_EMU_FAMILY_PACKAGE_FAILED"
        ),
    }
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::PublicKey { signing_key_env } => print_public_key(&signing_key_env),
        Command::Sign {
            binary,
            manifest,
            output,
            signing_key_env,
        } => sign(&binary, &manifest, &output, &signing_key_env),
        Command::NativeSign {
            binary,
            descriptor,
            output,
            target,
            signer_identity,
            signing_key_env,
        } => native_sign(
            &binary,
            &descriptor,
            &output,
            &target,
            &signer_identity,
            &signing_key_env,
        ),
        Command::StaticSign {
            archive,
            descriptor,
            output,
            target,
            signer_identity,
            signing_key_env,
        } => static_sign(StaticSignRequest {
            archive: &archive,
            descriptor: &descriptor,
            output: &output,
            target: &target,
            signer_identity: &signer_identity,
            signing_key_env: &signing_key_env,
        }),
        Command::AndroidSign {
            binary,
            manifest,
            descriptor,
            output_dir,
            package_name,
            version_code,
            abi,
            apk_signer_sha256,
            signer_identity,
            min_api,
            target_api,
            signing_key_env,
        } => android_sign(AndroidSignRequest {
            binary: &binary,
            manifest: manifest.as_deref(),
            descriptor: descriptor.as_deref(),
            output_dir: &output_dir,
            package_name: &package_name,
            version_code,
            abi: &abi,
            apk_signer_sha256: &apk_signer_sha256,
            signer_identity: &signer_identity,
            min_api,
            target_api,
            signing_key_env: &signing_key_env,
        }),
    }
}

fn print_public_key(signing_key_env: &str) -> Result<(), Box<dyn std::error::Error>> {
    let secret =
        env::var(signing_key_env).map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_MISSING")?;
    let secret =
        hex::decode(secret.trim()).map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_ENCODING")?;
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_LENGTH")?;
    println!(
        "{}",
        hex::encode(SigningKey::from_bytes(&secret).verifying_key().to_bytes())
    );
    Ok(())
}

struct StaticSignRequest<'a> {
    archive: &'a Path,
    descriptor: &'a Path,
    output: &'a Path,
    target: &'a str,
    signer_identity: &'a str,
    signing_key_env: &'a str,
}

struct AndroidSignRequest<'a> {
    binary: &'a Path,
    manifest: Option<&'a Path>,
    descriptor: Option<&'a Path>,
    output_dir: &'a Path,
    package_name: &'a str,
    version_code: u64,
    abi: &'a str,
    apk_signer_sha256: &'a str,
    signer_identity: &'a str,
    min_api: u32,
    target_api: u32,
    signing_key_env: &'a str,
}

fn sign(
    binary_path: &Path,
    manifest_path: &Path,
    output_path: &Path,
    signing_key_env: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if output_path == manifest_path {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_MUST_BE_DISTINCT".into());
    }
    let manifest_metadata = fs::metadata(manifest_path)?;
    let binary_metadata = fs::metadata(binary_path)?;
    if !manifest_metadata.is_file()
        || manifest_metadata.len() > MAX_MANIFEST_BYTES
        || !binary_metadata.is_file()
        || binary_metadata.len() == 0
        || binary_metadata.len() > MAX_PLUGIN_BYTES
    {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_INPUT_BOUNDS".into());
    }
    let binary = fs::read(binary_path)?;
    let mut manifest = load_and_bind_manifest(binary_path, manifest_path, &binary)?;
    sign_manifest(&mut manifest, signing_key_env)?;
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    write_new_atomic(output_path, &encoded)?;
    Ok(())
}

fn native_sign(
    binary_path: &Path,
    descriptor_path: &Path,
    output_path: &Path,
    target: &str,
    signer_identity: &str,
    signing_key_env: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if output_path == binary_path || output_path == descriptor_path {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_MUST_BE_DISTINCT".into());
    }
    let binary = read_bounded_file(binary_path, MAX_PLUGIN_BYTES)?;
    validate_native_library(&binary, target)?;
    let descriptor_bytes = read_bounded_file(descriptor_path, MAX_MANIFEST_BYTES)?;
    let descriptor: LegacyFamilyPluginDescriptor = serde_json::from_slice(&descriptor_bytes)?;
    descriptor.validate()?;
    let loaded = inspect_dynamic_family_descriptor(binary_path)?;
    if descriptor != loaded {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_DESCRIPTOR_MISMATCH".into());
    }
    let mut manifest = manifest_from_descriptor(&binary, descriptor_path, signer_identity, target)?;
    sign_manifest(&mut manifest, signing_key_env)?;
    write_new_atomic(output_path, &serde_json::to_vec_pretty(&manifest)?)
}

fn validate_native_library(binary: &[u8], target: &str) -> Result<(), Box<dyn std::error::Error>> {
    let object = object::File::parse(binary)?;
    let (format, architecture) = match target {
        "x86_64-pc-windows-msvc" => (BinaryFormat::Pe, Architecture::X86_64),
        "x86_64-unknown-linux-gnu" => (BinaryFormat::Elf, Architecture::X86_64),
        "aarch64-unknown-linux-gnu" => (BinaryFormat::Elf, Architecture::Aarch64),
        "x86_64-apple-darwin" => (BinaryFormat::MachO, Architecture::X86_64),
        "aarch64-apple-darwin" => (BinaryFormat::MachO, Architecture::Aarch64),
        _ => return Err("ASTRA_EMU_DESKTOP_TARGET_UNSUPPORTED".into()),
    };
    if object.format() != format || object.architecture() != architecture || !object.is_64() {
        return Err("ASTRA_EMU_DESKTOP_LIBRARY_BINARY_IDENTITY".into());
    }
    Ok(())
}

fn static_sign(request: StaticSignRequest<'_>) -> Result<(), Box<dyn std::error::Error>> {
    if request.output == request.archive || request.output == request.descriptor {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_MUST_BE_DISTINCT".into());
    }
    let architecture = validate_ios_target(request.target)?;
    let archive = read_bounded_file(request.archive, MAX_PLUGIN_BYTES)?;
    validate_apple_static_archive(&archive, architecture)?;
    let mut manifest = manifest_from_descriptor(
        &archive,
        request.descriptor,
        request.signer_identity,
        request.target,
    )?;
    sign_manifest(&mut manifest, request.signing_key_env)?;
    write_new_atomic(request.output, &serde_json::to_vec_pretty(&manifest)?)
}

fn validate_ios_target(target: &str) -> Result<Architecture, Box<dyn std::error::Error>> {
    match target {
        "aarch64-apple-ios" | "aarch64-apple-ios-sim" => Ok(Architecture::Aarch64),
        "x86_64-apple-ios" => Ok(Architecture::X86_64),
        _ => Err("ASTRA_EMU_IOS_TARGET_UNSUPPORTED".into()),
    }
}

fn validate_apple_static_archive(
    bytes: &[u8],
    expected_architecture: Architecture,
) -> Result<(), Box<dyn std::error::Error>> {
    let archive = ArchiveFile::parse(bytes)?;
    let mut matching_objects = 0_usize;
    for member in archive.members() {
        let member = member?;
        let data = member.data(bytes)?;
        let Ok(object) = object::File::parse(data) else {
            // Rust archives also contain metadata members. They are not native
            // code and therefore do not participate in architecture binding.
            continue;
        };
        if object.format() != BinaryFormat::MachO || object.architecture() != expected_architecture
        {
            return Err("ASTRA_EMU_IOS_ARCHIVE_BINARY_IDENTITY".into());
        }
        matching_objects = matching_objects
            .checked_add(1)
            .ok_or("ASTRA_EMU_IOS_ARCHIVE_OBJECT_BOUNDS")?;
        if matching_objects > 1_000_000 {
            return Err("ASTRA_EMU_IOS_ARCHIVE_OBJECT_BOUNDS".into());
        }
    }
    if matching_objects == 0 {
        return Err("ASTRA_EMU_IOS_ARCHIVE_BINARY_IDENTITY".into());
    }
    Ok(())
}

fn android_sign(request: AndroidSignRequest<'_>) -> Result<(), Box<dyn std::error::Error>> {
    if request.min_api < 26 || request.target_api != 36 || request.min_api > request.target_api {
        return Err("ASTRA_EMU_ANDROID_API_CONTRACT".into());
    }
    if !matches!(request.abi, "arm64-v8a" | "x86_64") {
        return Err("ASTRA_EMU_ANDROID_ABI_UNSUPPORTED".into());
    }
    validate_java_package_name(request.package_name)?;
    let binary = read_bounded_file(request.binary, MAX_PLUGIN_BYTES)?;
    validate_android_elf(&binary, request.abi)?;
    let mut family = match request.manifest {
        Some(path) => load_and_bind_manifest(request.binary, path, &binary)?,
        None => manifest_from_descriptor(
            &binary,
            request
                .descriptor
                .ok_or("ASTRA_EMU_FAMILY_PACKAGE_DESCRIPTOR_MISSING")?,
            request.signer_identity,
            match request.abi {
                "arm64-v8a" => "aarch64-linux-android",
                "x86_64" => "x86_64-linux-android",
                _ => return Err("ASTRA_EMU_ANDROID_ABI_UNSUPPORTED".into()),
            },
        )?,
    };
    family.native_manifest_hash = None;
    family.signature_hex.clear();
    let signer_digest: Hash256 = request
        .apk_signer_sha256
        .parse()
        .map_err(|_| "ASTRA_EMU_ANDROID_APK_SIGNER_HASH")?;
    let native = AndroidNativePluginManifest {
        schema: "astra.emu.android_native_plugin_manifest.v1".into(),
        apk_signer_digest: signer_digest,
        package_name: request.package_name.into(),
        version_code: request.version_code,
        abi: request.abi.into(),
        family_id: family.family_id.clone(),
        library_file_name: "libastra_emu_fvp.so".into(),
        library_hash: family.binary_hash,
        family_base_identity_hash: family_base_identity_hash(&family)?,
        min_api: request.min_api,
        target_api: request.target_api,
    };
    family.native_manifest_hash = Some(native.identity_hash()?);
    sign_manifest(&mut family, request.signing_key_env)?;

    let family_bytes = serde_json::to_vec_pretty(&family)?;
    let native_bytes = serde_json::to_vec_pretty(&native)?;
    write_new_directory_atomic(
        request.output_dir,
        &[
            ("manifest.json", &family_bytes),
            ("native-manifest.json", &native_bytes),
        ],
    )
}

fn validate_android_elf(binary: &[u8], abi: &str) -> Result<(), Box<dyn std::error::Error>> {
    let object = object::File::parse(binary)?;
    let expected = match abi {
        "arm64-v8a" => Architecture::Aarch64,
        "x86_64" => Architecture::X86_64,
        _ => return Err("ASTRA_EMU_ANDROID_ABI_UNSUPPORTED".into()),
    };
    if object.format() != BinaryFormat::Elf || object.architecture() != expected || !object.is_64()
    {
        return Err("ASTRA_EMU_ANDROID_LIBRARY_BINARY_IDENTITY".into());
    }
    Ok(())
}

fn manifest_from_descriptor(
    binary: &[u8],
    descriptor_path: &Path,
    signer_identity: &str,
    target: &str,
) -> Result<FamilyPluginManifest, Box<dyn std::error::Error>> {
    let descriptor: LegacyFamilyPluginDescriptor =
        serde_json::from_slice(&read_bounded_file(descriptor_path, MAX_MANIFEST_BYTES)?)?;
    descriptor.validate()?;
    Ok(FamilyPluginManifest {
        schema: "astra.emu.native_plugin_manifest.v1".into(),
        family_id: descriptor.family_id.0,
        plugin_id: descriptor.plugin_id,
        provider_id: descriptor.provider_id,
        engine_version: descriptor.engine_version,
        rustc_fingerprint: descriptor.rustc_fingerprint,
        feature_fingerprint: descriptor.feature_fingerprint,
        abi_fingerprint: descriptor.abi_fingerprint,
        binary_hash: Hash256::from_sha256(binary),
        signer_identity: signer_identity.into(),
        signature_algorithm: "ed25519-v1".into(),
        signature_hex: String::new(),
        package_eligible: true,
        supported_targets: vec![target.into()],
        native_manifest_hash: None,
    })
}

fn load_and_bind_manifest(
    binary_path: &Path,
    manifest_path: &Path,
    binary: &[u8],
) -> Result<FamilyPluginManifest, Box<dyn std::error::Error>> {
    let manifest_bytes = read_bounded_file(manifest_path, MAX_MANIFEST_BYTES)?;
    let mut manifest: FamilyPluginManifest = serde_json::from_slice(&manifest_bytes)?;
    let descriptor = inspect_dynamic_family_descriptor(binary_path)?;
    if descriptor.family_id.0 != manifest.family_id
        || descriptor.plugin_id != manifest.plugin_id
        || descriptor.provider_id != manifest.provider_id
        || descriptor.engine_version != manifest.engine_version
        || descriptor.rustc_fingerprint != manifest.rustc_fingerprint
        || descriptor.feature_fingerprint != manifest.feature_fingerprint
        || descriptor.abi_fingerprint != manifest.abi_fingerprint
    {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_DESCRIPTOR_MISMATCH".into());
    }
    if manifest.signature_algorithm != "ed25519-v1" {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_SIGNATURE_ALGORITHM".into());
    }
    manifest.binary_hash = Hash256::from_sha256(binary);
    manifest.signature_hex.clear();
    Ok(manifest)
}

fn sign_manifest(
    manifest: &mut FamilyPluginManifest,
    signing_key_env: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let secret =
        env::var(signing_key_env).map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_MISSING")?;
    let secret =
        hex::decode(secret.trim()).map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_ENCODING")?;
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_SIGNING_KEY_LENGTH")?;
    let public_key = env::var("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX")
        .map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_PUBLIC_KEY_MISSING")?;
    let public_key = hex::decode(public_key.trim())
        .map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_PUBLIC_KEY_ENCODING")?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| "ASTRA_EMU_FAMILY_PACKAGE_PUBLIC_KEY_LENGTH")?;
    if SigningKey::from_bytes(&secret).verifying_key().to_bytes() != public_key {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_KEYPAIR_MISMATCH".into());
    }
    sign_manifest_with_key(manifest, &secret)
}

fn sign_manifest_with_key(
    manifest: &mut FamilyPluginManifest,
    secret: &[u8; 32],
) -> Result<(), Box<dyn std::error::Error>> {
    let signing_key = SigningKey::from_bytes(secret);
    manifest.signature_hex = hex::encode(
        signing_key
            .sign(&canonical_family_manifest_bytes(manifest)?)
            .to_bytes(),
    );
    let verifier = Ed25519FamilySignatureVerifier::new([(
        manifest.signer_identity.clone(),
        signing_key.verifying_key().to_bytes(),
    )])?;
    verifier.verify_manifest_identity(manifest)?;
    Ok(())
}

fn read_bounded_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_INPUT_BOUNDS".into());
    }
    Ok(fs::read(path)?)
}

fn validate_java_package_name(value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut parts = value.split('.');
    let valid = value.len() <= 255
        && value.contains('.')
        && parts.all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
                && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        });
    if !valid {
        return Err("ASTRA_EMU_ANDROID_PACKAGE_NAME".into());
    }
    Ok(())
}

fn write_new_directory_atomic(
    path: &Path,
    files: &[(&str, &[u8])],
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_EXISTS".into());
    }
    let parent = path
        .parent()
        .ok_or("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_PARENT")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_NAME")?;
    let temporary = parent.join(format!(".{name}.tmp"));
    fs::create_dir(&temporary)?;
    let result = (|| {
        for (name, bytes) in files {
            if name.contains(['/', '\\']) {
                return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_NAME".into());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(temporary.join(name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_EXISTS".into());
    }
    let parent = path
        .parent()
        .ok_or("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_PARENT")?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("ASTRA_EMU_FAMILY_PACKAGE_OUTPUT_NAME")?;
    let temporary = parent.join(format!(".{file_name}.tmp"));
    if temporary.exists() {
        return Err("ASTRA_EMU_FAMILY_PACKAGE_TEMP_EXISTS".into());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use astra_emu_manager_core::{
        Ed25519FamilySignatureVerifier, PinnedStaticFamilyVerifier,
        StaticFamilyRegistrationVerifier,
    };
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    use super::*;

    fn descriptor(path: &Path) {
        fs::write(
            path,
            serde_json::to_vec(&json!({
                "family_id": "fvp",
                "plugin_id": "astra.emu.fvp",
                "provider_id": "astra.emu.family.fvp",
                "engine_version": "0.1.0",
                "rustc_fingerprint": "sha256.rustc",
                "feature_fingerprint": "sha256.features",
                "abi_fingerprint": "sha256.abi",
                "supported_formats": ["fvp.hcb"],
                "permissions": ["vfs.read"],
                "report_redaction": "astra.emu.redaction.v1",
                "license": "MPL-2.0"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn static_manifest_is_bound_to_archive_and_verifies() {
        let directory = tempfile::tempdir().unwrap();
        let descriptor_path = directory.path().join("descriptor.json");
        descriptor(&descriptor_path);
        let archive = b"deterministic-static-archive-fixture";
        let mut manifest = manifest_from_descriptor(
            archive,
            &descriptor_path,
            "astra.release.family",
            "aarch64-apple-ios",
        )
        .unwrap();
        let secret = [7_u8; 32];
        sign_manifest_with_key(&mut manifest, &secret).unwrap();

        let signing_key = SigningKey::from_bytes(&secret);
        let signature = Ed25519FamilySignatureVerifier::new([(
            "astra.release.family".into(),
            signing_key.verifying_key().to_bytes(),
        )])
        .unwrap();
        PinnedStaticFamilyVerifier::new(signature, Hash256::from_sha256(archive))
            .verify_static_registration(&manifest)
            .unwrap();
    }

    #[test]
    fn static_manifest_descriptor_rejects_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let descriptor_path = directory.path().join("descriptor.json");
        descriptor(&descriptor_path);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&descriptor_path).unwrap()).unwrap();
        value["unexpected"] = json!(true);
        fs::write(&descriptor_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(manifest_from_descriptor(
            b"archive",
            &descriptor_path,
            "astra.release.family",
            "aarch64-apple-ios"
        )
        .is_err());
    }

    #[test]
    fn static_sign_rejects_unknown_target_and_empty_archive() {
        assert!(validate_ios_target("wasm32-unknown-unknown").is_err());
        let directory = tempfile::tempdir().unwrap();
        let empty = directory.path().join("empty.rlib");
        fs::write(&empty, []).unwrap();
        assert!(read_bounded_file(&empty, MAX_PLUGIN_BYTES).is_err());
    }

    #[test]
    fn native_target_catalog_is_explicit_and_rejects_cross_format_bytes() {
        assert!(validate_native_library(b"not-an-object", "x86_64-pc-windows-msvc").is_err());
        assert!(validate_native_library(b"not-an-object", "wasm32-unknown-unknown").is_err());
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn windows_pe_binary_identity_accepts_a_native_executable() {
        let executable = fs::read(std::env::current_exe().unwrap()).unwrap();
        validate_native_library(&executable, "x86_64-pc-windows-msvc").unwrap();
    }
}
