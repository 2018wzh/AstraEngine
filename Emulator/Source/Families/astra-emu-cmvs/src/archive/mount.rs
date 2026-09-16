use super::*;

impl CmvsArchive {
    pub fn mount(
        mount_id: String,
        prefix: String,
        configured_archives: Vec<(String, PathBuf)>,
        scheme: CmvsSchemeProfile,
        private_profile_hash: Hash256,
        mount_profile_hash: Hash256,
    ) -> Result<Self, CoreError> {
        Self::mount_with_cache(
            mount_id,
            prefix,
            configured_archives,
            Vec::new(),
            scheme,
            private_profile_hash,
            mount_profile_hash,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mount_with_cache(
        mount_id: String,
        prefix: String,
        configured_archives: Vec<(String, PathBuf)>,
        configured_loose: Vec<(String, PathBuf)>,
        scheme: CmvsSchemeProfile,
        private_profile_hash: Hash256,
        mount_profile_hash: Hash256,
        cache: Option<PlaintextCache>,
    ) -> Result<Self, CoreError> {
        scheme
            .validate()
            .map_err(|code| invalid(code, "CMVS scheme is invalid"))?;
        if prefix != "cmvs:/"
            || configured_archives.is_empty()
            || configured_archives.len() > MAX_ARCHIVES
        {
            return Err(invalid(
                "ASTRA_EMU_CMVS_MOUNT_ARCHIVES",
                "CMVS mount has an invalid archive set",
            ));
        }
        let mut archives = Vec::with_capacity(configured_archives.len());
        let mut entries = BTreeMap::new();
        let mut entry_ids = BTreeSet::new();
        for (role, path) in configured_archives {
            let metadata = std::fs::metadata(&path).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_ARCHIVE_SOURCE",
                    "CMVS archive source is unavailable",
                )
            })?;
            if metadata.len() == 0 {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_ARCHIVE_EMPTY",
                    "CMVS declared archive source is empty",
                ));
            }
            let (header, index) = read_index(&path, metadata.len())?;
            if header.version != CmvsCpzVersion::Cpz5 {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_CPZ_VERSION",
                    "CMVS v1 mount currently requires CPZ5 sources",
                ));
            }
            let parsed = parse_cpz5_index(&header, &index, &scheme)?;
            let source_hash = hash_file(&path)?;
            let archive_index = archives.len();
            for (ordinal, descriptor) in parsed.into_iter().enumerate() {
                if descriptor.stored_size == 0 || descriptor.stored_size > MAX_ENTRY_BYTES {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_ENTRY_SIZE",
                        "CMVS CPZ5 entry exceeds the bounded VFS policy",
                    ));
                }
                let end = descriptor
                    .offset
                    .checked_add(descriptor.stored_size)
                    .ok_or_else(|| {
                        invalid("ASTRA_EMU_CMVS_ENTRY_RANGE", "CMVS entry range overflowed")
                    })?;
                if end > metadata.len() {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_ENTRY_RANGE",
                        "CMVS entry range exceeds its source archive",
                    ));
                }
                let uri = format!("{prefix}{}", descriptor.relative_path);
                validate_archive_uri(&prefix, &uri)?;
                let entry_id = format!("{role}-{ordinal}");
                if !entry_ids.insert(entry_id.clone()) || entries.contains_key(&uri) {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_ENTRY_DUPLICATE",
                        "CMVS archive set contains a duplicate URI or entry id",
                    ));
                }
                entries.insert(
                    uri.clone(),
                    MountedEntry {
                        uri,
                        entry_id,
                        archive: archive_index,
                        descriptor,
                    },
                );
                if entries.len() > MAX_ENTRIES {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_ENTRY_LIMIT",
                        "CMVS archive set exceeds the mounted entry budget",
                    ));
                }
            }
            archives.push(ArchiveFile {
                role,
                path,
                byte_size: metadata.len(),
                source_stamp: source_stamp(&metadata)?,
                source_hash,
                header,
            });
        }
        let mut loose = BTreeMap::new();
        for (role, path) in configured_loose {
            if loose.len() >= MAX_ARCHIVES {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_LOOSE_LIMIT",
                    "CMVS loose resource set exceeds the bounded role budget",
                ));
            }
            let metadata = std::fs::metadata(&path).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_LOOSE_SOURCE",
                    "CMVS loose resource source is unavailable",
                )
            })?;
            if metadata.len() == 0 || metadata.len() > MAX_ENTRY_BYTES {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_LOOSE_SIZE",
                    "CMVS loose resource is empty or exceeds the bounded VFS policy",
                ));
            }
            let uri = format!("{prefix}loose/{role}");
            validate_archive_uri(&prefix, &uri)?;
            if entries.contains_key(&uri) || loose.contains_key(&role) {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_LOOSE_DUPLICATE",
                    "CMVS loose resource role collides with a mounted URI",
                ));
            }
            let source_hash = hash_file(&path)?;
            let source_media_kind = loose_media_kind(&path)?;
            loose.insert(
                role.clone(),
                LooseSource {
                    role,
                    path,
                    media_kind: source_media_kind.into(),
                    byte_size: metadata.len(),
                    source_stamp: source_stamp(&metadata)?,
                    source_hash,
                },
            );
        }
        let reader_hash = Hash256::from_sha256(
            &archives
                .iter()
                .flat_map(|archive| archive.source_hash.as_bytes().iter().copied())
                .chain(
                    loose
                        .values()
                        .flat_map(|source| source.source_hash.as_bytes().iter().copied()),
                )
                .collect::<Vec<_>>(),
        );
        let manifest = ArchiveManifest {
            schema: ARCHIVE_MANIFEST_SCHEMA.into(),
            family_id: CMVS_FAMILY_ID.into(),
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: CMVS_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: CMVS_DECRYPT_PROVIDER_ID.into(),
            private_profile_hash,
            mount_profile_hash,
            sources: archives
                .iter()
                .map(|archive| ArchiveSource {
                    source_id: archive.role.clone(),
                    archive_role: Some(archive.role.clone()),
                    byte_size: archive.byte_size,
                    part_count: 1,
                    source_hash: archive.source_hash,
                })
                .chain(loose.values().map(|source| ArchiveSource {
                    source_id: format!("loose-{}", source.role),
                    archive_role: Some(format!("loose-{}", source.role)),
                    byte_size: source.byte_size,
                    part_count: 1,
                    source_hash: source.source_hash,
                }))
                .collect(),
            entries: entries
                .values()
                .map(|entry| ArchiveEntry {
                    uri: entry.uri.clone(),
                    entry_id: entry.entry_id.clone(),
                    source_id: archives[entry.archive].role.clone(),
                    source_offset: entry.descriptor.offset,
                    stored_size: entry.descriptor.stored_size,
                    decoded_size: entry.descriptor.stored_size,
                    source_hash: archives[entry.archive].source_hash,
                    content_hash: None,
                    method: "cpz5-encrypted".into(),
                    media_kind: media_kind(&entry.descriptor.relative_path).into(),
                })
                .chain(loose.values().map(|source| ArchiveEntry {
                    uri: format!("{prefix}loose/{}", source.role),
                    entry_id: format!("loose-{}", source.role),
                    source_id: format!("loose-{}", source.role),
                    source_offset: 0,
                    stored_size: source.byte_size,
                    decoded_size: source.byte_size,
                    source_hash: source.source_hash,
                    content_hash: None,
                    method: "loose".into(),
                    media_kind: source.media_kind.clone(),
                }))
                .collect(),
        };
        manifest.validate(MAX_ENTRIES)?;
        Ok(Self {
            mount_id,
            prefix,
            manifest,
            archives,
            entries,
            loose,
            scheme,
            private_profile_hash,
            cache,
            ephemeral_entry: Mutex::new(None),
        })
    }
}
