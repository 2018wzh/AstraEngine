use super::*;

pub(super) fn read_index(
    path: &PathBuf,
    length: u64,
) -> Result<(CmvsCpzHeader, Vec<u8>), CoreError> {
    let mut source = File::open(path).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_SOURCE_IO",
            "CMVS archive could not be opened",
        )
    })?;
    let mut raw_header = [0; 0x48];
    let header_size = source
        .read(&mut raw_header)
        .map_err(|_| invalid("ASTRA_EMU_CMVS_SOURCE_IO", "CMVS header could not be read"))?;
    let header = parse_cpz_header(&raw_header[..header_size], length)?;
    let size = usize::try_from(header.index_size).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_SIZE",
            "CMVS index exceeds platform bounds",
        )
    })?;
    let mut index = vec![0; size];
    source
        .seek(SeekFrom::Start(header.index_offset))
        .map_err(|_| invalid("ASTRA_EMU_CMVS_SOURCE_IO", "CMVS index seek failed"))?;
    source.read_exact(&mut index).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_SOURCE_SHORT_READ",
            "CMVS index source was truncated",
        )
    })?;
    Ok((header, index))
}

pub(super) fn verify_archive(archive: &ArchiveFile) -> Result<(), CoreError> {
    verify_archive_stamp(archive)?;
    if hash_file(&archive.path)? != archive.source_hash {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "CMVS source changed after mount",
        ));
    }
    Ok(())
}

pub(super) fn verify_archive_stamp(archive: &ArchiveFile) -> Result<(), CoreError> {
    let metadata = std::fs::metadata(&archive.path).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "CMVS source is unavailable after mount",
        )
    })?;
    if source_stamp(&metadata)? != archive.source_stamp {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "CMVS source metadata changed after mount",
        ));
    }
    Ok(())
}

pub(super) fn verify_loose_stamp(source: &LooseSource) -> Result<(), CoreError> {
    let metadata = std::fs::metadata(&source.path).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "CMVS loose resource is unavailable after mount",
        )
    })?;
    if source_stamp(&metadata)? != source.source_stamp {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "CMVS loose resource metadata changed after mount",
        ));
    }
    Ok(())
}

pub(super) fn source_stamp(metadata: &std::fs::Metadata) -> Result<SourceStamp, CoreError> {
    Ok(SourceStamp {
        byte_size: metadata.len(),
        modified: metadata.modified().map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_SOURCE_METADATA",
                "CMVS source modification time is unavailable",
            )
        })?,
    })
}

pub(super) fn hash_file(path: &PathBuf) -> Result<Hash256, CoreError> {
    let mut source = File::open(path).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_SOURCE_IO",
            "CMVS source could not be opened",
        )
    })?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_SOURCE_IO", "CMVS source could not be read"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(Hash256::from_sha256(&hash.finalize()))
}
