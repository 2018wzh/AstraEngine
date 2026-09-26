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
        if !astra_core::is_safe_symbol(&mount_id) {
            return Err(invalid(
                "ASTRA_EMU_VFS_MANIFEST",
                "archive mount identity is invalid",
            ));
        }
        let mut roles = BTreeSet::new();
        for role in configured_archives
            .iter()
            .map(|(role, _)| role.clone())
            .chain(
                configured_loose
                    .iter()
                    .map(|(role, _)| format!("loose-{role}")),
            )
        {
            if !astra_core::is_safe_symbol(&role) || !roles.insert(role) {
                return Err(invalid(
                    "ASTRA_EMU_VFS_SOURCE",
                    "archive source identity is invalid or duplicated",
                ));
            }
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
            tracing::debug!(
                event = "cmvs.archive.mount.begin",
                archive_index = archives.len(),
                source_bytes = metadata.len()
            );
            if header.version != CmvsCpzVersion::Cpz5 {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_CPZ_VERSION",
                    "CMVS v1 mount currently requires CPZ5 sources",
                ));
            }
            let parsed = parse_cpz5_index(&header, &index, &scheme)?;
            tracing::debug!(
                event = "cmvs.archive.index.ready",
                archive_index = archives.len(),
                entry_count = parsed.len()
            );
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
                let uri = format!("{prefix}archive/{role}/{}", descriptor.relative_path);
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
                        ordinal,
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
            let mounted = ArchiveFile {
                role,
                path,
                byte_size: metadata.len(),
                source_stamp: source_stamp(&metadata)?,
                source_hash: OnceLock::new(),
                header,
            };
            verify_archive_stamp(&mounted)?;
            archives.push(mounted);
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
            if entries.contains_key(&uri)
                || loose.contains_key(&role)
                || !entry_ids.insert(format!("loose-{role}"))
            {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_LOOSE_DUPLICATE",
                    "CMVS loose resource role collides with a mounted URI",
                ));
            }
            let source_media_kind = loose_media_kind(&path)?;
            loose.insert(
                role.clone(),
                LooseSource {
                    role,
                    path,
                    media_kind: source_media_kind.into(),
                    byte_size: metadata.len(),
                    source_stamp: source_stamp(&metadata)?,
                    source_hash: OnceLock::new(),
                },
            );
        }
        if entries.len() + loose.len() > MAX_ENTRIES {
            return Err(invalid(
                "ASTRA_EMU_CMVS_ENTRY_LIMIT",
                "mounted entry budget exceeded",
            ));
        }
        Ok(Self {
            game_root: None,
            mount_id,
            prefix,
            mount_profile_hash,
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
