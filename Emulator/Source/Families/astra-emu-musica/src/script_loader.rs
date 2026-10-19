use crate::{
    parse_sc_with_encoding,
    scene::{error, read_asset},
    MusicaMountedVfs, ScOpcodeCatalog, ScScript, ScriptEncoding,
};
use astra_core::Hash256;
use astra_emu_family_api::FamilyResult;

const MAX_SCRIPT_BYTES: usize = 16 * 1024 * 1024;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;
const MAX_INCLUDE_DEPTH: usize = 32;

pub(crate) struct LoadedScript {
    pub hash: Hash256,
    pub script: ScScript,
}

pub(crate) fn load_script(
    archive: &MusicaMountedVfs,
    uri: &str,
    primary: ScriptEncoding,
) -> FamilyResult<LoadedScript> {
    let bytes = read_expanded_script(archive, uri)?;
    let encoding = ScriptEncoding::detect(&bytes, primary);
    let script = parse_sc_with_encoding(&bytes, &ScOpcodeCatalog::observed_musica(), encoding)
        .map_err(|_| {
            tracing::debug!(event = "astra.emu.musica.script.parse_failed");
            error("ASTRA_EMU_MUSICA_SCRIPT", "script cannot be parsed")
        })?;
    Ok(LoadedScript {
        hash: Hash256::from_sha256(&bytes),
        script,
    })
}

/// Reads through the same bounded loose/PAZ/include pipeline as native playback.
/// This does not execute the script or create an audio/GPU device.
pub fn read_expanded_script(archive: &MusicaMountedVfs, uri: &str) -> FamilyResult<Vec<u8>> {
    let mut read = |uri: &str| {
        let loose = archive
            .loose_scripts
            .keys()
            .find(|entry| entry.eq_ignore_ascii_case(uri));
        if let Some(canonical) = loose {
            let (root, relative) = &archive.loose_scripts[canonical];
            return crate::resources::read_resource(root, relative, MAX_SCRIPT_BYTES as u64)
                .map_err(crate::scene::core_error);
        }
        let canonical = resolve_script_uri(
            uri,
            archive
                .manifest()
                .entries
                .iter()
                .map(|entry| entry.uri.as_str()),
        )?;
        read_asset(archive, canonical, MAX_SCRIPT_BYTES as u64).map(|bytes| bytes.to_vec())
    };
    expand(uri, &mut read)
}

#[derive(Debug, serde::Serialize)]
pub struct ScriptValidation {
    pub uri: String,
    pub encoding: ScriptEncoding,
    pub source_bytes: usize,
    pub lines: usize,
    pub valid: bool,
    pub diagnostic: Option<&'static str>,
    pub offset: Option<usize>,
    pub line: Option<usize>,
    pub target: Option<String>,
}

/// Safe diagnostics expose source locations and branch symbols, never source lines.
pub fn validate_musica_script(
    archive: &MusicaMountedVfs,
    uri: &str,
    primary: ScriptEncoding,
) -> FamilyResult<ScriptValidation> {
    let bytes = read_expanded_script(archive, uri)?;
    let encoding = ScriptEncoding::detect(&bytes, primary);
    let mut report = ScriptValidation {
        uri: uri.to_owned(),
        encoding,
        source_bytes: bytes.len(),
        lines: bytes.split_inclusive(|byte| *byte == b'\n').count(),
        valid: true,
        diagnostic: None,
        offset: None,
        line: None,
        target: None,
    };
    if let Err(cause) =
        parse_sc_with_encoding(&bytes, &ScOpcodeCatalog::observed_musica(), encoding)
    {
        use crate::ScParseError::*;
        report.valid = false;
        report.diagnostic = Some(match &cause {
            Encoding(_) => "encoding",
            OperandSchema(_) => "operand_schema",
            SourceInvariant(_) => "source_invariant",
            DuplicateLabel(_) => "duplicate_label",
            InvalidTarget(_) => "missing_target",
            DuplicateOpcode(_) => "duplicate_opcode",
        });
        match cause {
            Encoding(offset) | OperandSchema(offset) | SourceInvariant(offset) => {
                report.offset = Some(offset);
                report.line = Some(
                    1 + bytes[..offset.min(bytes.len())]
                        .iter()
                        .filter(|b| **b == b'\n')
                        .count(),
                );
            }
            DuplicateLabel(target) | InvalidTarget(target) => {
                report.target = Some(target);
            }
            DuplicateOpcode(_) => {}
        }
    }
    Ok(report)
}

fn resolve_script_uri<'a>(
    requested: &str,
    entries: impl Iterator<Item = &'a str>,
) -> FamilyResult<&'a str> {
    let mut matches = entries.filter(|entry| entry.eq_ignore_ascii_case(requested));
    let found = matches.next().ok_or_else(|| {
        error(
            "ASTRA_EMU_MUSICA_SCRIPT_NOT_FOUND",
            "script is absent from the mounted archive",
        )
    })?;
    if matches.next().is_some() {
        return Err(error(
            "ASTRA_EMU_MUSICA_SCRIPT_AMBIGUOUS",
            "script names collide under native case rules",
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod native_names {
    use super::*;
    #[test]
    fn resolves_native_case_without_choosing_between_colliding_entries() {
        let uri = "musica:/scr/Scene.SC";
        assert_eq!(
            resolve_script_uri(uri, ["musica:/scr/scene.sc"].into_iter()).unwrap(),
            "musica:/scr/scene.sc"
        );
        assert!(resolve_script_uri(uri, ["musica:/scr/other.sc"].into_iter()).is_err());
        assert!(resolve_script_uri(uri, [uri, "musica:/scr/scene.sc"].into_iter()).is_err());
    }
}

fn expand(
    uri: &str,
    read: &mut impl FnMut(&str) -> FamilyResult<Vec<u8>>,
) -> FamilyResult<Vec<u8>> {
    let mut output = Vec::new();
    expand_into(uri, read, &mut Vec::new(), &mut 0, &mut output)?;
    Ok(output)
}

fn expand_into(
    uri: &str,
    read: &mut impl FnMut(&str) -> FamilyResult<Vec<u8>>,
    stack: &mut Vec<String>,
    read_bytes: &mut usize,
    output: &mut Vec<u8>,
) -> FamilyResult<()> {
    if stack.len() >= MAX_INCLUDE_DEPTH {
        return Err(error(
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_DEPTH",
            "script include depth exceeded",
        ));
    }
    if stack
        .iter()
        .any(|current| current.eq_ignore_ascii_case(uri))
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_CYCLE",
            "script include cycle",
        ));
    }
    let source = read(uri)?;
    *read_bytes = read_bytes
        .checked_add(source.len())
        .filter(|bytes| *bytes <= MAX_READ_BYTES)
        .ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_SIZE",
                "script read budget exceeded",
            )
        })?;
    stack.push(uri.to_owned());
    for segment in source.split_inclusive(|byte| *byte == b'\n') {
        let line = segment.strip_suffix(b"\n").unwrap_or(segment);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line = trim(line);
        if !line.starts_with(b".include") {
            append(output, segment)?;
            continue;
        }
        let target = include_target(line).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_OPERAND",
                "include requires one safe direct .sc target",
            )
        })?;
        let target = format!("musica:/scr/{target}");
        let start = output.len();
        expand_into(&target, read, stack, read_bytes, output)?;
        if segment.ends_with(b"\n") && !output[start..].ends_with(b"\n") {
            append(output, b"\r\n")?;
        }
    }
    stack.pop();
    Ok(())
}

fn append(output: &mut Vec<u8>, bytes: &[u8]) -> FamilyResult<()> {
    if output
        .len()
        .checked_add(bytes.len())
        .is_none_or(|len| len > MAX_SCRIPT_BYTES)
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_SIZE",
            "expanded script exceeds source budget",
        ));
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

fn include_target(line: &[u8]) -> Option<&str> {
    let mut tokens = line.split(|byte| matches!(byte, b' ' | b'\t'));
    if tokens.next()? != b".include" {
        return None;
    }
    let target = std::str::from_utf8(tokens.next()?).ok()?;
    if target.is_empty()
        || target.len() > 256
        || tokens.next().is_some()
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mounted_aliases_and_loose_include_precedence_keep_native_roles() {
        let root = tempfile::tempdir().unwrap();
        crate::test_fixture::game(root.path(), b".end\r\n");
        crate::test_fixture::assets(
            root.path(),
            "scr",
            &[
                ("test.sc", b".include shared.sc\r\n.end\r\n"),
                ("shared.sc", b"; archive\r\n"),
                ("fallback.sc", b"; fallback\r\n"),
            ],
        );
        std::fs::rename(root.path().join("scr.paz"), root.path().join("cnscr.paz")).unwrap();
        std::fs::rename(root.path().join("sys.paz"), root.path().join("cnsys.paz")).unwrap();
        std::fs::create_dir(root.path().join("scripts")).unwrap();
        std::fs::write(
            root.path().join("scripts/shared.sc"),
            b"; loose\r\n.include fallback.sc\r\n",
        )
        .unwrap();
        let paths = crate::MusicaResourcePaths {
            scr_archive: "cnscr.paz".into(),
            sys_archive: "cnsys.paz".into(),
            script_directory: "scripts".into(),
        };
        let (archive, _) = crate::mount_musica_with_resources(
            root.path(),
            std::path::Path::new("key.toml"),
            &paths,
        )
        .unwrap();
        assert!(archive
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.uri == "musica:/scr/test.sc"));
        assert!(!archive
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.uri.contains("cnscr")));
        assert_eq!(
            read_expanded_script(&archive, "musica:/scr/TEST.sc").unwrap(),
            b"; loose\r\n; fallback\r\n.end\r\n"
        );
        let identity = archive.script_source_identity().unwrap();
        std::fs::write(root.path().join("scripts/shared.sc"), b"; changed\r\n").unwrap();
        assert_ne!(archive.script_source_identity().unwrap(), identity);
        std::fs::write(
            root.path().join("scripts/shared.sc"),
            vec![b';'; MAX_SCRIPT_BYTES + 1],
        )
        .unwrap();
        assert_eq!(
            read_expanded_script(&archive, "musica:/scr/test.sc")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_RESOURCE_BOUND"
        );
        let invalid = crate::MusicaResourcePaths {
            scr_archive: "../escape.paz".into(),
            ..Default::default()
        };
        assert!(crate::mount_musica_with_resources(
            root.path(),
            std::path::Path::new("key.toml"),
            &invalid
        )
        .is_err());
    }

    #[test]
    fn nested_includes_preserve_bytes_and_supply_missing_newlines() {
        let files = [
            (
                "musica:/scr/root.sc",
                b"; root\r\n.include a.sc\r\n.end\r\n".as_slice(),
            ),
            ("musica:/scr/a.sc", b".include b.sc\n.message 1   Hello"),
            ("musica:/scr/b.sc", b"; nested"),
        ];
        let output = expand(files[0].0, &mut |uri| {
            Ok(files
                .iter()
                .find(|(key, _)| *key == uri)
                .unwrap()
                .1
                .to_vec())
        })
        .unwrap();
        assert_eq!(
            output,
            b"; root\r\n; nested\r\n.message 1   Hello\r\n.end\r\n"
        );
    }

    #[test]
    fn malformed_include_targets_are_rejected() {
        for line in [
            ".include",
            ".include ../a.sc",
            ".include dir/a.sc",
            ".include a.sc extra",
            ".include  a.sc",
            ".include a.txt",
            ".includeX a.sc",
        ] {
            assert_eq!(
                expand("musica:/scr/root.sc", &mut |_| Ok(line.as_bytes().to_vec()))
                    .err()
                    .unwrap()
                    .code(),
                "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_OPERAND"
            );
        }
    }

    #[test]
    fn cycles_depth_and_expansion_are_bounded() {
        assert_eq!(
            expand("musica:/scr/root.sc", &mut |_| Ok(
                b".include ROOT.sc".to_vec()
            ))
            .err()
            .unwrap()
            .code(),
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_CYCLE"
        );
        let mut count = 0;
        assert_eq!(
            expand("musica:/scr/root.sc", &mut |_| {
                count += 1;
                Ok(format!(".include next{count}.sc").into_bytes())
            })
            .err()
            .unwrap()
            .code(),
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_DEPTH"
        );
        assert_eq!(count, MAX_INCLUDE_DEPTH);
        assert_eq!(
            expand("musica:/scr/root.sc", &mut |_| Ok(vec![
                b';';
                MAX_SCRIPT_BYTES + 1
            ]))
            .err()
            .unwrap()
            .code(),
            "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_SIZE"
        );
    }
}
