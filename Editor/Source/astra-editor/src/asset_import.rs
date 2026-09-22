use crate::project::Project;
use astra_asset::{AssetId, AssetSidecar};
use astra_cook::{DefaultMetadataImporter, ImportRequest};
use std::{
    io::Write,
    path::{Component, Path},
};

pub struct ImageImport {
    pub destination: String,
    pub sidecar_root: String,
    pub asset_id: String,
    pub license: String,
    pub profiles: Vec<String>,
}
pub struct PreparedImage {
    bytes: Vec<u8>,
    sidecar: AssetSidecar,
    sidecar_root: String,
}

/// Decode through the existing cook importer on the background executor.
pub fn prepare_image(source: &Path, settings: ImageImport) -> anyhow::Result<PreparedImage> {
    relative(&settings.destination)?;
    relative(&settings.sidecar_root)?;
    anyhow::ensure!(
        !settings.license.trim().is_empty(),
        "Enter the asset license"
    );
    anyhow::ensure!(
        !settings.profiles.is_empty(),
        "Project has no cook profiles"
    );
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    anyhow::ensure!(
        matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp"),
        "Import supports PNG, JPEG and WebP images"
    );
    anyhow::ensure!(
        Path::new(&settings.destination)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case(&extension)),
        "Destination must preserve the image extension"
    );
    let mut file = std::fs::File::open(source)?;
    anyhow::ensure!(
        file.metadata()?.len() <= 64 * 1024 * 1024,
        "Image exceeds 64 MiB import limit"
    );
    let mut bytes = Vec::new();
    use std::io::Read;
    (&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "Image exceeds import limit"
    );
    let mut staged = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()?;
    staged.write_all(&bytes)?;
    let audit = DefaultMetadataImporter::new("astra.import.image").import(ImportRequest {
        asset_id: AssetId::parse(&settings.asset_id)?,
        source_path: staged.path().to_owned(),
        asset_type: format!("image.{extension}"),
        license: settings.license,
        font: None,
        target_profiles: settings.profiles,
    })?;
    let mut sidecar = audit.sidecar;
    sidecar.source = settings.destination;
    Ok(PreparedImage {
        bytes,
        sidecar,
        sidecar_root: settings.sidecar_root,
    })
}

impl Project {
    /// Never overwrite a resource or sidecar. A collision requires a new destination/ID.
    pub fn import_image(&mut self, image: PreparedImage) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.asset_roots.contains(&image.sidecar_root),
            "Choose one of the project's asset roots"
        );
        anyhow::ensure!(
            image.sidecar.cook.target_profiles == self.profiles,
            "Project cook profiles changed"
        );
        for path in &self.content {
            if !path.ends_with(".astra-asset.yaml") {
                continue;
            }
            let old = AssetSidecar::from_yaml(&std::fs::read_to_string(self.root.join(path))?)?;
            anyhow::ensure!(
                old.id != image.sidecar.id,
                "Asset ID already exists; choose another ID or cancel"
            );
        }
        let name = image
            .sidecar
            .id
            .as_str()
            .strip_prefix("asset:/")
            .unwrap()
            .replace('/', "__");
        let sidecar_path = format!("{}/{}.astra-asset.yaml", image.sidecar_root, name);
        let destination = checked_destination(&self.root, &image.sidecar.source)?;
        let metadata = checked_destination(&self.root, &sidecar_path)?;
        anyhow::ensure!(
            !destination.exists() && !metadata.exists(),
            "Name already exists; choose another destination/ID or cancel"
        );
        let mut payload = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
        payload.write_all(&image.bytes)?;
        payload.as_file().sync_all()?;
        let mut sidecar = tempfile::NamedTempFile::new_in(metadata.parent().unwrap())?;
        sidecar.write_all(image.sidecar.to_yaml()?.as_bytes())?;
        sidecar.as_file().sync_all()?;
        payload.persist_noclobber(&destination)?;
        if let Err(error) = sidecar.persist_noclobber(&metadata) {
            // The source was created by this operation, never an existing asset.
            std::fs::remove_file(&destination)?;
            return Err(error.into());
        }
        self.content.push(sidecar_path.clone());
        self.content.sort();
        Ok(sidecar_path)
    }
}
fn relative(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_empty()
            && !path.contains(['\\', ':'])
            && Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Asset destination must be a project-relative path"
    );
    Ok(())
}
fn checked_destination(root: &Path, path: &str) -> anyhow::Result<std::path::PathBuf> {
    relative(path)?;
    let path = Path::new(path);
    let mut directory = root.to_owned();
    for component in path.parent().unwrap().components() {
        directory.push(component);
        if !directory.exists() {
            std::fs::create_dir(&directory)?;
        }
        anyhow::ensure!(
            directory.canonicalize()?.starts_with(root),
            "Asset destination escapes the project"
        );
    }
    let destination = directory.join(path.file_name().unwrap());
    anyhow::ensure!(
        std::fs::symlink_metadata(&destination).is_err(),
        "Name already exists; choose another destination or cancel"
    );
    Ok(destination)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_decodes_hashes_reopens_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Scripts")).unwrap();
        std::fs::create_dir(dir.path().join("AssetSidecars")).unwrap();
        std::fs::write(
            dir.path().join("Scripts/main.astra"),
            "story main #@id main\nstate start #@id start\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("project.yaml"), "nativevn:\n  sources: [Scripts]\n  asset_roots: [AssetSidecars]\n  profiles: [classic]\n").unwrap();
        let external = tempfile::tempdir().unwrap();
        let source = external.path().join("image.png");
        image::RgbaImage::new(2, 2).save(&source).unwrap();
        let settings = || ImageImport {
            destination: "Images/image.png".into(),
            sidecar_root: "AssetSidecars".into(),
            asset_id: "asset:/images/image".into(),
            license: "CC0".into(),
            profiles: vec!["classic".into()],
        };
        let mut project = Project::open(&dir.path().join("project.yaml")).unwrap();
        let sidecar = project
            .import_image(prepare_image(&source, settings()).unwrap())
            .unwrap();
        let imported = std::fs::read(dir.path().join("Images/image.png")).unwrap();
        let metadata =
            AssetSidecar::from_yaml(&std::fs::read_to_string(dir.path().join(&sidecar)).unwrap())
                .unwrap();
        assert_eq!(
            metadata.source_hash,
            Some(astra_core::Hash256::from_sha256(&imported))
        );
        assert!(metadata.validate().is_empty());
        assert!(project
            .import_image(prepare_image(&source, settings()).unwrap())
            .is_err());
        assert_eq!(
            std::fs::read(dir.path().join("Images/image.png")).unwrap(),
            imported
        );
        assert!(Project::open(&dir.path().join("project.yaml"))
            .unwrap()
            .content
            .contains(&sidecar));
        let mut unsafe_settings = settings();
        unsafe_settings.destination = "../escape.png".into();
        assert!(prepare_image(&source, unsafe_settings).is_err());
        std::fs::write(&source, b"not an image").unwrap();
        assert!(prepare_image(&source, settings()).is_err());
    }
}
