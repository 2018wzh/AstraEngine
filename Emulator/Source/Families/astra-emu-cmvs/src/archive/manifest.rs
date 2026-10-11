use super::*;
impl CmvsArchive {
    /// Builds a content-bound manifest on demand. Normal playback never requests it.
    pub fn manifest(&self) -> Result<ArchiveManifest, CoreError> {
        let archives = &self.archives;
        let loose = &self.loose;
        let entries = &self.entries;
        let prefix = &self.prefix;
        let archive_hashes = archives
            .iter()
            .map(|source| archive_hash(source, false))
            .collect::<Result<Vec<_>, _>>()?;
        let loose_hashes = loose
            .values()
            .map(|source| Ok((source.role.clone(), loose_hash(source, false)?)))
            .collect::<Result<BTreeMap<_, _>, CoreError>>()?;
        let reader_hash = Hash256::from_sha256(
            &archives
                .iter()
                .enumerate()
                .flat_map(|(index, _archive)| archive_hashes[index].as_bytes().iter().copied())
                .chain(
                    loose
                        .values()
                        .flat_map(|source| loose_hashes[&source.role].as_bytes().iter().copied()),
                )
                .collect::<Vec<_>>(),
        );
        let manifest = ArchiveManifest {
            schema: ARCHIVE_MANIFEST_SCHEMA.into(),
            family_id: CMVS_FAMILY_ID.into(),
            mount_id: self.mount_id.clone(),
            prefix: self.prefix.clone(),
            reader_id: CMVS_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: CMVS_DECRYPT_PROVIDER_ID.into(),
            private_profile_hash: self.private_profile_hash,
            mount_profile_hash: self.mount_profile_hash,
            sources: archives
                .iter()
                .enumerate()
                .map(|(index, archive)| ArchiveSource {
                    source_id: archive.role.clone(),
                    archive_role: Some(archive.role.clone()),
                    byte_size: archive.byte_size,
                    part_count: 1,
                    source_revision: archive_hashes[index],
                })
                .chain(loose.values().map(|source| ArchiveSource {
                    source_id: format!("loose-{}", source.role),
                    archive_role: Some(format!("loose-{}", source.role)),
                    byte_size: source.byte_size,
                    part_count: 1,
                    source_revision: loose_hashes[&source.role],
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
                    source_revision: archive_hashes[entry.archive],
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
                    source_revision: loose_hashes[&source.role],
                    content_hash: None,
                    method: "loose".into(),
                    media_kind: source.media_kind.clone(),
                }))
                .collect(),
        };
        manifest.validate(MAX_ENTRIES)?;
        Ok(manifest)
    }
}
