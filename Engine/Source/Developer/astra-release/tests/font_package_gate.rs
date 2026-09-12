use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    FontPackageEntry, FontPackageManifest, UnicodeRange, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{PackageBuildRequest, PackageBuilder, SectionPayload};
use astra_release::{CheckStatus, PackageValidateRequest, ReleaseValidator};

fn package_with_font(manifest_target: &str) -> Vec<u8> {
    let font = include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
        .to_vec();
    let hash = Hash256::from_sha256(&font);
    let section = SectionPayload::raw("asset.font.ui", "astra.cooked_asset.v1", font.clone());
    let mut request =
        PackageBuildRequest::fixture("com.example.release-font", "classic", vec![section]);
    request.media_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.media_manifest.v1",
        "font_manifest_required": true,
        "font_manifest_section": "media.font_manifest"
    }))
    .unwrap();
    request.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": { "kind": "package_section", "section_id": "package.manifest" },
            "targets": ["native-smoke-game"],
            "profiles": ["classic"]
        }],
        "entries": [{
            "vfs_uri": "package:/asset/font/ui",
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": "asset.font.ui" },
            "offset": 0,
            "size": font.len(),
            "hash": hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }],
        "whiteouts": []
    }))
    .unwrap();
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
        target: manifest_target.into(),
        profile: "classic".into(),
        provider_binding: "astra.vfs.package".into(),
        fonts: vec![FontPackageEntry {
            asset_id: "asset:/font/ui/poppins-regular".into(),
            uri: VfsUri::parse("package:/asset/font/ui").unwrap(),
            family: "Poppins".into(),
            face_index: 0,
            hash,
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: vec![UnicodeRange {
                start: 0,
                end: 0x036f,
            }],
            targets: vec!["native-smoke-game".into()],
            profiles: vec!["classic".into()],
        }],
    };
    request.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&manifest).unwrap(),
    ));
    PackageBuilder::build(request).unwrap().into_bytes()
}

fn font_check(package: Vec<u8>) -> astra_release::ReleaseCheckRecord {
    ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: package,
            profile: "classic".into(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".into()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap()
        .checks
        .into_iter()
        .find(|check| check.id == "media.font_package")
        .unwrap()
}

#[test]
fn release_gate_validates_package_vfs_font_authority_and_target_drift() {
    let valid = font_check(package_with_font("native-smoke-game"));
    assert_eq!(valid.status, CheckStatus::Pass);
    assert!(valid.evidence.iter().any(|item| item.key == "font_count"));

    let drift = font_check(package_with_font("other-game"));
    assert_eq!(drift.status, CheckStatus::Blocked);
    assert_eq!(drift.diagnostic.unwrap().code, "ASTRA_TEXT_PACKAGE_INVALID");
}
