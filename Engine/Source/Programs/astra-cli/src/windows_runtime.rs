use super::{bundle_file, CliError, StandaloneBundleFile};
use object::{Architecture, BinaryFormat, Object, ObjectKind};
use std::{fs, path::Path};

const REQUIRED: [&str; 3] = ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"];

pub(super) fn bundle(runtime: &Path, out: &Path) -> Result<Vec<StandaloneBundleFile>, CliError> {
    for name in REQUIRED {
        if !runtime.join(name).is_file() {
            return Err(format!("ASTRA_WINDOWS_RUNTIME_MISSING: {name}").into());
        }
    }
    let mut paths = fs::read_dir(runtime)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut libraries = Vec::new();
    for path in paths {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
        {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("ASTRA_WINDOWS_RUNTIME_FILENAME")?;
        let bytes = fs::read(&path)?;
        let image = object::File::parse(bytes.as_slice())
            .map_err(|_| format!("ASTRA_WINDOWS_RUNTIME_INVALID: {name}"))?;
        if image.format() != BinaryFormat::Pe
            || image.architecture() != Architecture::X86_64
            || image.kind() != ObjectKind::Dynamic
        {
            return Err(format!("ASTRA_WINDOWS_RUNTIME_INVALID: {name}").into());
        }
        libraries.push((name.to_owned(), bytes));
    }
    let mut files = Vec::new();
    for (name, bytes) in libraries {
        fs::write(out.join(&name), &bytes)?;
        files.push(bundle_file(&name, "windows_runtime", &bytes));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A PE32+ header fixture, never loaded or executed as a native library.
    fn dll_header(machine: u16, dll: bool) -> Vec<u8> {
        let mut bytes = vec![0; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[132..134].copy_from_slice(&machine.to_le_bytes());
        bytes[148..150].copy_from_slice(&240_u16.to_le_bytes());
        bytes[150..152].copy_from_slice(&(if dll { 0x2022_u16 } else { 0x22 }).to_le_bytes());
        bytes[152..154].copy_from_slice(&0x20b_u16.to_le_bytes());
        bytes
    }

    #[test]
    fn runtime_files_are_included_in_the_manifest_and_missing_or_corrupt_inputs_fail() {
        let source = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        assert!(bundle(source.path(), output.path())
            .unwrap_err()
            .to_string()
            .contains("MISSING"));
        for name in REQUIRED {
            fs::write(source.path().join(name), dll_header(0x8664, true)).unwrap();
        }
        fs::write(source.path().join("license.txt"), b"not a runtime library").unwrap();
        let manifest = bundle(source.path(), output.path()).unwrap();
        assert_eq!(manifest.len(), 3);
        for entry in manifest {
            assert_eq!(entry.role, "windows_runtime");
            let bytes = fs::read(output.path().join(&entry.path)).unwrap();
            assert_eq!(
                entry.hash,
                astra_core::Hash256::from_sha256(&bytes).to_string()
            );
            assert_eq!(entry.byte_size, bytes.len() as u64);
        }
        assert!(!output.path().join("license.txt").exists());
        fs::write(source.path().join(REQUIRED[0]), b"invalid").unwrap();
        assert!(bundle(source.path(), output.path())
            .unwrap_err()
            .to_string()
            .contains("INVALID"));
    }

    #[test]
    fn invalid_runtime_type_is_rejected_before_any_library_is_copied() {
        let source = tempfile::tempdir().unwrap();
        for name in REQUIRED {
            fs::write(source.path().join(name), dll_header(0x8664, true)).unwrap();
        }
        for invalid in [
            b"MZ truncated".to_vec(),
            dll_header(0xaa64, true),
            dll_header(0x8664, false),
        ] {
            let output = tempfile::tempdir().unwrap();
            // Sort after the required DLLs to catch validation during copying.
            fs::write(source.path().join("z_invalid.dll"), invalid).unwrap();
            assert!(bundle(source.path(), output.path()).is_err());
            assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
        }
    }
}
