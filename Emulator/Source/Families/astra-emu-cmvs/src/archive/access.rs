use super::*;

impl CmvsArchive {
    pub fn mount_id(&self) -> &str {
        &self.mount_id
    }

    pub fn manifest(&self) -> &ArchiveManifest {
        &self.manifest
    }

    pub fn validate_sources(&self) -> Result<(), CoreError> {
        for archive in &self.archives {
            verify_archive(archive)?;
        }
        for source in self.loose.values() {
            verify_loose_stamp(source)?;
        }
        Ok(())
    }

    pub fn read_dir(&self, uri: &str) -> Result<Vec<ArchiveNode>, CoreError> {
        validate_archive_directory_uri(&self.prefix, uri)?;
        let base = if uri.ends_with('/') {
            uri.to_owned()
        } else {
            format!("{uri}/")
        };
        let mut children = BTreeMap::new();
        if !self.loose.is_empty() {
            let loose_uri = format!("{}loose", self.prefix);
            if base == format!("{loose_uri}/") {
                for role in self.loose.keys() {
                    children.insert(
                        role.clone(),
                        ArchiveNode {
                            uri: format!("{loose_uri}/{role}"),
                            name: role.clone(),
                            kind: ArchiveNodeKind::File,
                        },
                    );
                }
                return Ok(children.into_values().collect());
            }
            if loose_uri.starts_with(&base) {
                let name = loose_uri[base.len()..]
                    .split('/')
                    .next()
                    .unwrap_or_default();
                if !name.is_empty() {
                    children.insert(
                        name.to_owned(),
                        ArchiveNode {
                            uri: format!("{base}{name}"),
                            name: name.to_owned(),
                            kind: ArchiveNodeKind::Directory,
                        },
                    );
                }
            }
        }
        for entry_uri in self
            .entries
            .keys()
            .filter(|candidate| candidate.starts_with(&base))
        {
            let suffix = &entry_uri[base.len()..];
            let name = suffix.split('/').next().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            children
                .entry(name.to_owned())
                .or_insert_with(|| ArchiveNode {
                    uri: format!("{base}{name}"),
                    name: name.to_owned(),
                    kind: if suffix.contains('/') {
                        ArchiveNodeKind::Directory
                    } else {
                        ArchiveNodeKind::File
                    },
                });
        }
        if children.is_empty() && uri != self.prefix {
            return Err(invalid(
                "ASTRA_EMU_VFS_NOT_FOUND",
                "VFS directory was not found",
            ));
        }
        Ok(children.into_values().collect())
    }

    pub fn stat(&self, uri: &str) -> Result<ArchiveStat, CoreError> {
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: Some(format!("loose-{role}")),
                kind: ArchiveNodeKind::File,
                size: source.byte_size,
                archive_role: Some(format!("loose-{role}")),
                method: Some("loose".into()),
            });
        }
        if uri == format!("{}loose", self.prefix) && !self.loose.is_empty() {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        if uri == self.prefix
            || self
                .entries
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{}/", uri.trim_end_matches('/'))))
        {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(ArchiveStat {
            uri: uri.into(),
            entry_id: Some(entry.entry_id.clone()),
            kind: ArchiveNodeKind::File,
            size: entry.descriptor.stored_size,
            archive_role: Some(self.archives[entry.archive].role.clone()),
            method: Some("cpz5-encrypted".into()),
        })
    }

    pub fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<ArchiveReadResult, CoreError> {
        if length > ARCHIVE_MAX_READ_BYTES {
            return Err(invalid(
                "ASTRA_EMU_VFS_READ_LIMIT",
                "VFS read exceeds the bounded range limit",
            ));
        }
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            if offset > source.byte_size
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > source.byte_size)
            {
                return Err(invalid(
                    "ASTRA_EMU_VFS_READ_RANGE",
                    "VFS range is outside the loose resource",
                ));
            }
            let bytes = self.read_loose_range(source, offset, length)?;
            let eof = offset + length == source.byte_size;
            return Ok(ArchiveReadResult {
                uri: uri.into(),
                offset,
                bytes: bytes.into(),
                eof,
                cache_hit: false,
            });
        }
        let entry = self.entry(uri)?;
        if offset > entry.descriptor.stored_size
            || offset
                .checked_add(length)
                .is_none_or(|end| end > entry.descriptor.stored_size)
        {
            return Err(invalid(
                "ASTRA_EMU_VFS_READ_RANGE",
                "VFS range is outside the entry",
            ));
        }
        let (decoded, cache_hit) = self.decode_entry(entry)?;
        let start = usize::try_from(offset).map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_READ_RANGE",
                "VFS range exceeds platform bounds",
            )
        })?;
        let end = start
            .checked_add(usize::try_from(length).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_READ_RANGE",
                    "VFS range exceeds platform bounds",
                )
            })?)
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_READ_RANGE", "VFS range overflowed"))?;
        let eof = end == decoded.len();
        let bytes = OwnedByteBuffer::from_owner((decoded, start..end), |(owner, range)| {
            &owner[range.clone()]
        });
        Ok(ArchiveReadResult {
            uri: uri.into(),
            offset,
            bytes,
            eof,
            cache_hit,
        })
    }

    pub fn open_stream(&self, uri: &str) -> Result<Box<dyn ArchiveStream>, CoreError> {
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            let bytes = self.read_loose_range(source, 0, source.byte_size)?;
            return Ok(Box::new(Cursor::new(bytes)));
        }
        let (decoded, _) = self.decode_entry(self.entry(uri)?)?;
        Ok(Box::new(Cursor::new(decoded)))
    }
}
