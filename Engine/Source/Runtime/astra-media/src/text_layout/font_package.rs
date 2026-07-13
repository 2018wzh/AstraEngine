use astra_asset::{ResolveContext, VfsBackendKind, VfsManifest, VfsSourceRef, VfsUri};
use astra_core::Hash256;
use astra_package::{ContainerCryptoProvider, PackageManifest, PackageReader};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

use super::{
    contract::{FontBindingContext, PackagedFont, TextLayoutConfig, UnicodeRange},
    provider::CosmicTextLayoutProvider,
};

pub const FONT_PACKAGE_MANIFEST_SCHEMA: &str = "astra.font_manifest.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontPackageManifest {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub provider_binding: String,
    pub fonts: Vec<FontPackageEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontPackageEntry {
    pub asset_id: String,
    pub uri: VfsUri,
    pub family: String,
    pub face_index: u32,
    pub hash: Hash256,
    pub license_id: String,
    pub subset: Option<String>,
    pub coverage: Vec<UnicodeRange>,
    pub targets: Vec<String>,
    pub profiles: Vec<String>,
}

impl CosmicTextLayoutProvider {
    pub fn from_package(
        package: &PackageReader,
        manifest_section: &str,
        context: FontBindingContext,
        config: TextLayoutConfig,
    ) -> Result<Self, MediaError> {
        load_from_package(package, manifest_section, context, config, None)
    }

    pub fn from_package_with_crypto(
        package: &PackageReader,
        manifest_section: &str,
        context: FontBindingContext,
        config: TextLayoutConfig,
        crypto: &dyn ContainerCryptoProvider,
    ) -> Result<Self, MediaError> {
        load_from_package(package, manifest_section, context, config, Some(crypto))
    }
}

fn load_from_package(
    package: &PackageReader,
    manifest_section: &str,
    context: FontBindingContext,
    config: TextLayoutConfig,
    crypto: Option<&dyn ContainerCryptoProvider>,
) -> Result<CosmicTextLayoutProvider, MediaError> {
    let entry = package
        .container()
        .section_entry(manifest_section)
        .ok_or_else(|| {
            package_error(
                "ASTRA_TEXT_PACKAGE_MANIFEST_MISSING",
                "font manifest section is missing",
            )
        })?;
    if entry.schema != FONT_PACKAGE_MANIFEST_SCHEMA {
        return Err(package_error(
            "ASTRA_TEXT_PACKAGE_MANIFEST_SCHEMA",
            "font manifest section schema is unsupported",
        ));
    }
    let manifest_bytes = read_section(package, manifest_section, 4 * 1024 * 1024, crypto)?;
    let manifest: FontPackageManifest = serde_json::from_slice(&manifest_bytes).map_err(|_| {
        package_error(
            "ASTRA_TEXT_PACKAGE_MANIFEST_DECODE",
            "font manifest JSON is invalid",
        )
    })?;
    if manifest.schema != FONT_PACKAGE_MANIFEST_SCHEMA
        || manifest.target != context.target
        || manifest.profile != context.profile
        || manifest.provider_binding.trim().is_empty()
        || manifest.fonts.is_empty()
    {
        return Err(package_error(
            "ASTRA_TEXT_PACKAGE_MANIFEST_IDENTITY",
            "font manifest target, profile, provider, or font set is invalid",
        ));
    }
    let package_manifest: PackageManifest = package
        .container()
        .decode_postcard("package.manifest")
        .map_err(|_| {
            package_error(
                "ASTRA_TEXT_PACKAGE_IDENTITY",
                "package manifest could not be decoded",
            )
        })?;
    if package_manifest.profile != context.profile {
        return Err(package_error(
            "ASTRA_TEXT_PACKAGE_PROFILE",
            "package profile does not match the font binding context",
        ));
    }
    let vfs_bytes = read_section(package, "asset.vfs_manifest", 16 * 1024 * 1024, crypto)?;
    let vfs: VfsManifest = serde_json::from_slice(&vfs_bytes).map_err(|_| {
        package_error(
            "ASTRA_TEXT_PACKAGE_VFS_DECODE",
            "asset VFS manifest JSON is invalid",
        )
    })?;
    let mut fonts = Vec::with_capacity(manifest.fonts.len());
    for font in manifest.fonts {
        let prefix = vfs
            .prefixes
            .iter()
            .find(|prefix| prefix.prefix == font.uri.prefix())
            .ok_or_else(|| {
                package_error(
                    "ASTRA_TEXT_PACKAGE_VFS_PREFIX",
                    "font URI prefix is not registered",
                )
            })?;
        if prefix.backend != VfsBackendKind::Package {
            return Err(package_error(
                "ASTRA_TEXT_PACKAGE_VFS_BACKEND",
                "shipping font must resolve through the package VFS backend",
            ));
        }
        let resolve = ResolveContext {
            target: context.target.clone(),
            profile: context.profile.clone(),
            capability: prefix.backend.required_provider_capability().to_string(),
            provider_binding: manifest.provider_binding.clone(),
        };
        let resolved = vfs
            .resolve(&font.uri, &resolve)
            .map_err(MediaError::Diagnostics)?
            .ok_or_else(|| {
                package_error(
                    "ASTRA_TEXT_PACKAGE_FONT_UNRESOLVED",
                    "font URI has no eligible VFS entry",
                )
            })?;
        if resolved.media_kind != "font"
            || resolved
                .codec
                .as_deref()
                .is_some_and(|codec| !matches!(codec, "raw" | "zstd"))
            || resolved.hash != font.hash
        {
            return Err(package_error(
                "ASTRA_TEXT_PACKAGE_FONT_IDENTITY",
                "font VFS media kind, codec, or hash does not match its manifest",
            ));
        }
        let VfsSourceRef::PackageSection { section_id } = &resolved.source else {
            return Err(package_error(
                "ASTRA_TEXT_PACKAGE_FONT_SOURCE",
                "font VFS entry does not reference a package section",
            ));
        };
        let section = read_section(package, section_id, config.max_font_bytes, crypto)?;
        let start = usize::try_from(resolved.offset).map_err(|_| {
            package_error(
                "ASTRA_TEXT_PACKAGE_FONT_RANGE",
                "font VFS offset does not fit host coordinates",
            )
        })?;
        let size = usize::try_from(resolved.size).map_err(|_| {
            package_error(
                "ASTRA_TEXT_PACKAGE_FONT_RANGE",
                "font VFS size does not fit host coordinates",
            )
        })?;
        let end = start.checked_add(size).ok_or_else(|| {
            package_error(
                "ASTRA_TEXT_PACKAGE_FONT_RANGE",
                "font VFS byte range overflows",
            )
        })?;
        let bytes = section.get(start..end).ok_or_else(|| {
            package_error(
                "ASTRA_TEXT_PACKAGE_FONT_RANGE",
                "font VFS byte range is outside its package section",
            )
        })?;
        if Hash256::from_sha256(bytes) != font.hash {
            return Err(package_error(
                "ASTRA_TEXT_PACKAGE_FONT_HASH",
                "resolved font bytes do not match the VFS hash",
            ));
        }
        fonts.push(PackagedFont {
            asset_id: font.asset_id,
            family: font.family,
            face_index: font.face_index,
            hash: font.hash,
            license_id: font.license_id,
            subset: font.subset,
            coverage: font.coverage,
            targets: font.targets,
            profiles: font.profiles,
            bytes: bytes.to_vec(),
        });
    }
    tracing::info!(
        target: "astra_media::text",
        event = "text.font_package.loaded",
        package_id = %package_manifest.package_id,
        target_id = %context.target,
        profile = %context.profile,
        font_count = fonts.len(),
    );
    CosmicTextLayoutProvider::new(context, fonts, config)
}

fn read_section(
    package: &PackageReader,
    section_id: &str,
    max_len: usize,
    crypto: Option<&dyn ContainerCryptoProvider>,
) -> Result<Vec<u8>, MediaError> {
    let result = if let Some(crypto) = crypto {
        package
            .container()
            .read_bounded_with_crypto(section_id, max_len, crypto)
    } else {
        package.container().read_bounded(section_id, max_len)
    };
    result.map_err(|_| {
        package_error(
            "ASTRA_TEXT_PACKAGE_SECTION_READ",
            "font package section could not be read within its declared bound",
        )
    })
}

fn package_error(code: &str, message: &str) -> MediaError {
    MediaError::message(format!("{code}: {message}"))
}
