use super::*;

pub(crate) fn vfs_checks(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> Vec<ReleaseCheckRecord> {
    vec![
        vfs_uri_format_check(package),
        vfs_prefix_registry_check(package),
        vfs_package_mount_check(package, selected_target, profile),
        vfs_overlay_mount_check(package),
        vfs_catalog_check(package),
    ]
}

pub(crate) fn vfs_uri_format_check(package: &PackageReader) -> ReleaseCheckRecord {
    if package.has_section("asset.registry") {
        return package_blocked(
            "vfs.uri_format",
            "ASTRA_VFS_ASSET_REGISTRY_REMOVED",
            "legacy asset.registry section is not accepted after VFS migration",
            vec![evidence("section", "asset.registry")],
        );
    }

    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.uri_format"),
    };
    for entry in &manifest.entries {
        if let Err(err) = VfsUri::parse(entry.uri.as_str()) {
            return package_blocked(
                "vfs.uri_format",
                "ASTRA_VFS_URI_INVALID",
                format!("VFS entry URI is invalid: {err}"),
                vec![evidence("vfs_uri", entry.uri.as_str())],
            );
        }
    }
    for whiteout in &manifest.whiteouts {
        if let Err(err) = VfsUri::parse(whiteout.uri.as_str()) {
            return package_blocked(
                "vfs.uri_format",
                "ASTRA_VFS_URI_INVALID",
                format!("VFS whiteout URI is invalid: {err}"),
                vec![evidence("vfs_uri", whiteout.uri.as_str())],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.uri_format".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "all VFS locators use provider:/path format".to_string(),
        diagnostic: None,
        evidence: vec![evidence("entry_count", manifest.entries.len())],
    }
}

pub(crate) fn vfs_prefix_registry_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.prefix_registry"),
    };
    if let Some(path) = forbidden_vfs_report_field(package, "asset.vfs_manifest") {
        return package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_REPORT_PAYLOAD_LEAK",
            "asset.vfs_manifest contains a local-root or payload-like field",
            vec![evidence("field", path)],
        );
    }
    if manifest.prefixes.is_empty() {
        return package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_PREFIX_MISSING",
            "asset.vfs_manifest must declare at least one prefix",
            vec![evidence("section", "asset.vfs_manifest")],
        );
    }
    let validation = manifest.validate();
    if let Some(diagnostic) = validation.first() {
        return ReleaseCheckRecord {
            id: "vfs.prefix_registry".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "asset VFS prefix registry is invalid".to_string(),
            diagnostic: Some(diagnostic.clone()),
            evidence: vec![evidence("prefix_count", manifest.prefixes.len())],
        };
    }

    let providers = match vfs_registered_providers(package) {
        Ok(providers) => providers,
        Err(check) => return (*check).with_id("vfs.prefix_registry"),
    };
    let mut seen_prefixes = BTreeSet::new();
    for prefix in &manifest.prefixes {
        if !seen_prefixes.insert(prefix.prefix.as_str()) {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PREFIX_CONFLICT",
                format!("VFS prefix {} is declared more than once", prefix.prefix),
                vec![evidence("prefix", &prefix.prefix)],
            );
        }
        let Some(provider) = providers.get(prefix.provider_id.as_str()) else {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_MISSING",
                format!(
                    "VFS prefix {} provider {} is not registered in vfs_provider slot",
                    prefix.prefix, prefix.provider_id
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                ],
            );
        };
        if !provider.packaged {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_UNPACKAGED",
                format!(
                    "VFS prefix {} provider {} is not packaged eligible",
                    prefix.prefix, prefix.provider_id
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                ],
            );
        }
        let required_capability = prefix.backend.required_provider_capability();
        if !vfs_capability_matches(&provider.capability, required_capability) {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_CAPABILITY_MISMATCH",
                format!(
                    "VFS prefix {} provider {} does not match backend capability {}",
                    prefix.prefix, prefix.provider_id, required_capability
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                    evidence("required_capability", required_capability),
                ],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.prefix_registry".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "VFS prefixes bind to packaged vfs_provider registrations".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("prefix_count", manifest.prefixes.len()),
            evidence("provider_count", providers.len()),
        ],
    }
}

pub(crate) fn vfs_package_mount_check(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.package_mount"),
    };
    let layers = manifest
        .layers
        .iter()
        .map(|layer| (layer.layer_id.as_str(), layer))
        .collect::<BTreeMap<_, _>>();
    for entry in &manifest.entries {
        if let Some(layer) = layers.get(entry.layer_id.as_str()) {
            if !layer.profiles.is_empty()
                && !layer.profiles.iter().any(|profile| !profile.is_empty())
            {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_PROFILE_INVALID",
                    "VFS layer profile eligibility contains an empty profile",
                    vec![evidence("layer_id", &entry.layer_id)],
                );
            }
        }
        if let VfsSourceRef::PackageSection { section_id } = &entry.source {
            let Some(section) = package.container().section_entry(section_id) else {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_PACKAGE_SECTION_MISSING",
                    format!("VFS package entry references missing section {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                    ],
                );
            };
            let end = entry.offset.checked_add(entry.size);
            if entry.size == 0 || end.is_none_or(|end| end > section.decoded_length) {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_BOUNDS_INVALID",
                    format!("VFS package entry bounds exceed section {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                        evidence("entry_size", entry.size),
                        evidence("section_size", section.decoded_length),
                    ],
                );
            }
            let section_bytes = match package.container().read_section(section_id) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return package_blocked(
                        "vfs.package_mount",
                        "ASTRA_VFS_PACKAGE_SECTION_READ",
                        format!("VFS package section {section_id} could not be read: {error}"),
                        vec![evidence("section_id", section_id)],
                    )
                }
            };
            let (Ok(start), Ok(size)) =
                (usize::try_from(entry.offset), usize::try_from(entry.size))
            else {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_BOUNDS_INVALID",
                    "VFS package entry bounds do not fit host coordinates",
                    vec![evidence("vfs_uri", entry.uri.as_str())],
                );
            };
            let end = start + size;
            if Hash256::from_sha256(&section_bytes[start..end]) != entry.hash {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_HASH_MISMATCH",
                    format!("VFS package entry hash does not match its section range {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                    ],
                );
            }
        }
    }

    let target = selected_target
        .map(str::to_string)
        .or_else(|| {
            package
                .container()
                .read_bounded("target.manifest", 256 * 1024)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<TargetManifest>(&bytes).ok())
                .and_then(|manifest| {
                    manifest
                        .targets
                        .into_iter()
                        .find(|target| target.kind == TargetKind::Game)
                        .map(|target| target.id)
                })
        })
        .unwrap_or_default();
    if target.is_empty() {
        return package_blocked(
            "vfs.package_mount",
            "ASTRA_VFS_RESOLVE_TARGET_MISSING",
            "VFS release validation requires a selected game target",
            vec![],
        );
    }
    let mut uris = BTreeSet::new();
    for entry in &manifest.entries {
        let Some(layer) = layers.get(entry.layer_id.as_str()) else {
            continue;
        };
        if (!layer.targets.is_empty() && !layer.targets.iter().any(|value| value == &target))
            || (!layer.profiles.is_empty() && !layer.profiles.iter().any(|value| value == profile))
        {
            continue;
        }
        if !uris.insert(entry.uri.clone()) {
            continue;
        }
        let prefix = manifest
            .prefixes
            .iter()
            .find(|prefix| prefix.prefix == entry.uri.prefix())
            .expect("validated VFS manifest prefix must exist");
        let required_capability = prefix.backend.required_provider_capability();
        if !prefix
            .capabilities
            .iter()
            .any(|capability| capability == required_capability)
        {
            return package_blocked(
                "vfs.package_mount",
                "ASTRA_VFS_CAPABILITY_MISSING",
                "VFS prefix must declare its backend capability",
                vec![evidence("prefix", &prefix.prefix)],
            );
        }
        let context = ResolveContext {
            target: target.clone(),
            profile: profile.to_string(),
            capability: required_capability.to_string(),
            provider_binding: prefix.provider_id.clone(),
        };
        if let Err(diagnostics) = manifest.resolve(&entry.uri, &context) {
            return ReleaseCheckRecord {
                id: "vfs.package_mount".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "VFS entry is not uniquely resolvable for the release context".to_string(),
                diagnostic: diagnostics.into_iter().next(),
                evidence: vec![
                    evidence("vfs_uri", entry.uri.as_str()),
                    evidence("target", &target),
                    evidence("profile", profile),
                ],
            };
        }
    }

    ReleaseCheckRecord {
        id: "vfs.package_mount".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "package-backed VFS entries match container section bounds and hashes".to_string(),
        diagnostic: None,
        evidence: vec![evidence("entry_count", manifest.entries.len())],
    }
}

pub(crate) fn vfs_overlay_mount_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.overlay_mount"),
    };
    let layer_ids = manifest
        .layers
        .iter()
        .map(|layer| layer.layer_id.as_str())
        .collect::<BTreeSet<_>>();
    for whiteout in &manifest.whiteouts {
        if !layer_ids.contains(whiteout.layer_id.as_str())
            || whiteout.allowlist_id.trim().is_empty()
            || whiteout.reason.trim().is_empty()
        {
            return package_blocked(
                "vfs.overlay_mount",
                "ASTRA_VFS_WHITEOUT_UNAUTHORIZED",
                "VFS whiteout requires a valid layer, allowlist and reason",
                vec![
                    evidence("vfs_uri", whiteout.uri.as_str()),
                    evidence("layer_id", &whiteout.layer_id),
                ],
            );
        }
    }
    ReleaseCheckRecord {
        id: "vfs.overlay_mount".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "VFS overlay layers and whiteouts are explicitly authorized".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("layer_count", manifest.layers.len()),
            evidence("whiteout_count", manifest.whiteouts.len()),
        ],
    }
}

pub(crate) fn vfs_catalog_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.catalog"),
    };
    let catalog = match decode_asset_catalog(package) {
        Ok(catalog) => catalog,
        Err(check) => return (*check).with_id("vfs.catalog"),
    };
    if let Some(path) = forbidden_vfs_report_field(package, "asset.catalog") {
        return package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_PAYLOAD_LEAK",
            "asset.catalog contains a local-root or payload-like field",
            vec![evidence("field", path)],
        );
    }
    if catalog.schema != "astra.asset_catalog.v1" {
        return package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_SCHEMA",
            "asset.catalog schema must be astra.asset_catalog.v1",
            vec![evidence("schema", catalog.schema)],
        );
    }
    let entry_uris = manifest
        .entries
        .iter()
        .map(|entry| entry.uri.as_str())
        .collect::<BTreeSet<_>>();
    let mut asset_ids = BTreeSet::new();
    for asset in &catalog.assets {
        if asset.asset_id.trim().is_empty() || !asset_ids.insert(asset.asset_id.as_str()) {
            return package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_ASSET_ID",
                "asset.catalog asset ids must be non-empty and unique",
                vec![evidence("asset_id", &asset.asset_id)],
            );
        }
        if !entry_uris.contains(asset.uri.as_str()) {
            return package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_URI_MISSING",
                format!(
                    "asset.catalog references VFS URI {} without an entry",
                    asset.uri
                ),
                vec![evidence("vfs_uri", asset.uri.as_str())],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.catalog".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "asset catalog references package VFS entries".to_string(),
        diagnostic: None,
        evidence: vec![evidence("asset_count", catalog.assets.len())],
    }
}
