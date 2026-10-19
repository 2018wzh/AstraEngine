use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Physical sources do not rename archive roles or the stable musica:/ URIs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicaResourcePaths {
    #[serde(default)]
    pub scr_archive: String,
    #[serde(default)]
    pub sys_archive: String,
    #[serde(default)]
    pub script_directory: String,
}

pub(crate) fn game_identity(
    archive: &crate::MusicaMountedVfs,
    translation: Option<astra_core::Hash256>,
    mode: crate::TranslationMode,
    font: Option<astra_core::Hash256>,
    subtitles: Option<astra_core::Hash256>,
) -> Result<astra_core::Hash256, CoreError> {
    let bytes = serde_json::to_vec(&(
        archive.manifest(),
        archive.script_source_identity()?,
        translation,
        mode,
        font,
        subtitles,
    ))
    .map_err(|_| {
        CoreError::invalid(
            "ASTRA_EMU_MUSICA_IDENTITY",
            "archive identity cannot be encoded",
        )
    })?;
    Ok(astra_core::Hash256::from_sha256(&bytes))
}

pub(crate) fn relative_resource(path: &str) -> Result<(), CoreError> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(CoreError::invalid(
            "ASTRA_EMU_MUSICA_RESOURCE_PATH",
            "resource path must be game-relative",
        ));
    }
    Ok(())
}

pub(crate) fn resource_file(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    relative_resource(relative)?;
    astra_emu_sdk::resolve_game_file(root, relative).map_err(|_| {
        CoreError::invalid(
            "ASTRA_EMU_MUSICA_RESOURCE_PATH",
            "resource file is missing or outside the game directory",
        )
    })
}

pub(crate) fn read_resource(root: &Path, relative: &str, limit: u64) -> Result<Vec<u8>, CoreError> {
    use std::io::Read;
    let path = resource_file(root, relative)?;
    let file = std::fs::File::open(path).map_err(|_| {
        CoreError::invalid(
            "ASTRA_EMU_MUSICA_RESOURCE_READ",
            "resource cannot be opened",
        )
    })?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|_| {
        CoreError::invalid("ASTRA_EMU_MUSICA_RESOURCE_READ", "resource cannot be read")
    })?;
    if bytes.len() as u64 > limit {
        return Err(CoreError::invalid(
            "ASTRA_EMU_MUSICA_RESOURCE_BOUND",
            "resource exceeds the supported byte bound",
        ));
    }

    Ok(bytes)
}

/// Load the actual configured face. No system-family substitution is permitted.
pub(crate) fn load_font(
    root: &Path,
    relative: &str,
) -> Result<Option<std::sync::Arc<astra_text::PackagedFont>>, CoreError> {
    if relative.is_empty() {
        return Ok(None);
    }
    let bytes = std::sync::Arc::new(read_resource(root, relative, 64 * 1024 * 1024)?);
    let mut database = fontdb::Database::new();
    let ids = database.load_font_source(fontdb::Source::Binary(bytes.clone()));
    let face = ids
        .iter()
        .filter_map(|id| database.face(*id))
        .find(|face| face.index == 0)
        .ok_or_else(|| {
            CoreError::invalid(
                "ASTRA_EMU_MUSICA_FONT",
                "configured font has no readable first face",
            )
        })?;
    let family = face
        .families
        .first()
        .map(|family| family.0.clone())
        .filter(|family| !family.is_empty())
        .ok_or_else(|| {
            CoreError::invalid(
                "ASTRA_EMU_MUSICA_FONT",
                "configured font has no internal family name",
            )
        })?;
    let coverage = astra_text::font_unicode_coverage(&bytes, 0).map_err(|_| {
        CoreError::invalid(
            "ASTRA_EMU_MUSICA_FONT",
            "configured font coverage cannot be read",
        )
    })?;
    drop(database);
    let bytes = std::sync::Arc::unwrap_or_clone(bytes);
    Ok(Some(std::sync::Arc::new(astra_text::PackagedFont {
        asset_id: "asset:/font/musica/configured".into(),
        family,
        face_index: 0,
        hash: astra_core::Hash256::from_sha256(&bytes),
        license_id: "local-private".into(),
        subset: None,
        coverage,
        targets: vec!["astra-emu-musica".into()],
        profiles: vec!["musica.reference".into()],
        bytes,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_external_and_ambiguous_paths() {
        for path in [
            "", "../a", "a/../b", "/a", "C:/a", "a\\b", "a//b", "a/./b", "a\0b",
        ] {
            assert!(relative_resource(path).is_err());
        }
        assert!(relative_resource("localized/scripts").is_ok());
    }
    #[test]
    fn configured_font_uses_internal_family_and_fails_for_missing_or_invalid_files() {
        let root = tempfile::tempdir().unwrap();
        assert!(load_font(root.path(), "").unwrap().is_none());
        assert!(load_font(root.path(), "missing.ttf").is_err());
        std::fs::write(root.path().join("bad.ttf"), b"not a font").unwrap();
        assert!(load_font(root.path(), "bad.ttf").is_err());
        std::fs::write(
            root.path().join("font.ttf"),
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf"),
        )
        .unwrap();
        let font = load_font(root.path(), "font.ttf").unwrap().unwrap();
        assert!(font.family.contains("Noto Sans JP"));
        let mut renderer =
            crate::MusicaTextRenderer::with_font(crate::ScriptEncoding::Gbk, Some(font)).unwrap();
        assert!(renderer.commands(Some(("Public", None)), None).is_ok());
        assert_eq!(
            renderer
                .commands(Some(("\u{10ffff}", None)), None)
                .unwrap_err(),
            "ASTRA_EMU_MUSICA_FONT_MISSING_GLYPH"
        );
    }
}
