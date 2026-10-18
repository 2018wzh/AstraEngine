use super::*;
use std::{
    io,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

// A movie has either a byte substitution table or a repeating RC4-derived mask.
struct MovieCipher {
    substitution: Option<[u8; 256]>,
    mask: Vec<u8>,
}
impl MovieCipher {
    fn new(
        version: u8,
        entry: &ArchiveEntryDescriptor,
        xor: u8,
        password: &str,
    ) -> Result<Self, PazError> {
        let video_key = entry
            .video_key
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_VIDEO_KEY", "movie entry has no index key"))?;
        if video_key.len() != 256 {
            return Err(error(
                "ASTRA_EMU_MUSICA_VIDEO_KEY",
                "movie index key has an invalid size",
            ));
        }
        if version == 0 {
            let mut inverse = [0; 256];
            for (index, value) in video_key.iter().enumerate() {
                inverse[*value as usize] = index as u8;
            }
            let mut table = [0; 256];
            for (index, value) in table.iter_mut().enumerate() {
                *value = inverse[index ^ usize::from(xor)];
            }
            return Ok(Self {
                substitution: Some(table),
                mask: Vec::new(),
            });
        }
        let material = format!(
            "{} {:08X} {}",
            entry.name.to_lowercase(),
            entry.unpacked_size,
            password
        );
        let (entry_key, _, malformed) = SHIFT_JIS.encode(&material);
        if malformed || entry_key.is_empty() {
            return Err(error(
                "ASTRA_EMU_MUSICA_RC4_KEY",
                "movie RC4 key cannot be encoded as CP932",
            ));
        }
        let mut key = [0; 256];
        for (index, value) in key.iter_mut().enumerate() {
            *value = video_key[index] ^ entry_key[index % entry_key.len()];
        }
        let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_RC4_KEY",
                "movie RC4 key length is invalid",
            )
        })?;
        let length = entry.aligned_size.min(0x10000) as usize;
        if length == 0 {
            return Err(error(
                "ASTRA_EMU_MUSICA_VIDEO_SIZE",
                "movie transform is empty",
            ));
        }
        let mut mask = vec![0; length];
        cipher.apply_keystream(&mut mask);
        for byte in &mut mask {
            *byte ^= xor;
        }
        Ok(Self {
            substitution: None,
            mask,
        })
    }
    fn apply(&self, mut bytes: &mut [u8], offset: u64) {
        if let Some(table) = &self.substitution {
            for byte in bytes {
                *byte = table[usize::from(*byte)];
            }
            return;
        }
        let mut position = (offset % self.mask.len() as u64) as usize;
        while !bytes.is_empty() {
            let count = bytes.len().min(self.mask.len() - position);
            let (part, rest) = bytes.split_at_mut(count);
            for (byte, key) in part.iter_mut().zip(&self.mask[position..position + count]) {
                *byte ^= *key;
            }
            bytes = rest;
            position = 0;
        }
    }
}

struct MovieReader<'a> {
    archive: &'a ArchiveFile,
    part: usize,
    part_offset: u64,
    remaining: u64,
    position: u64,
    file: Option<File>,
    cipher: MovieCipher,
    stop: &'a AtomicBool,
}
impl Read for MovieReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.stop.load(Ordering::Acquire) {
            return Err(io::Error::other(cancelled()));
        }
        if output.is_empty() || self.remaining == 0 {
            return Ok(0);
        }
        while self.part < self.archive.parts.len()
            && self.part_offset >= self.archive.parts[self.part].length
        {
            self.part_offset -= self.archive.parts[self.part].length;
            self.part += 1;
            self.file = None;
        }
        let part = self.archive.parts.get(self.part).ok_or_else(|| {
            io::Error::other(error(
                "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
                "movie spans beyond its archive parts",
            ))
        })?;
        if self.file.is_none() {
            let mut file = File::open(&part.path).map_err(|cause| {
                io::Error::other(error("ASTRA_EMU_MUSICA_ARCHIVE_OPEN", cause.to_string()))
            })?;
            file.seek(SeekFrom::Start(self.part_offset))
                .map_err(|cause| {
                    io::Error::other(error("ASTRA_EMU_MUSICA_ARCHIVE_SEEK", cause.to_string()))
                })?;
            self.file = Some(file);
        }
        let count = (output.len() as u64)
            .min(self.remaining)
            .min(part.length - self.part_offset) as usize;
        let read = self
            .file
            .as_mut()
            .expect("opened current part")
            .read(&mut output[..count])?;
        if read == 0 {
            return Err(io::Error::other(error(
                "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
                "movie archive was truncated",
            )));
        }
        self.cipher.apply(&mut output[..read], self.position);
        self.position += read as u64;
        self.part_offset += read as u64;
        self.remaining -= read as u64;
        Ok(read)
    }
}
fn cancelled() -> PazError {
    error(
        "ASTRA_EMU_MUSICA_MOVIE_CANCELLED",
        "movie source preparation cancelled",
    )
}
fn stream_error(cause: io::Error) -> PazError {
    let os_error = cause.raw_os_error();
    let kind = cause.kind();
    match cause.into_inner() {
        Some(inner) => match inner.downcast::<PazError>() {
            Ok(cause) => *cause,
            Err(cause) => error("ASTRA_EMU_MUSICA_MOVIE_READ", cause.to_string()),
        },
        None => error(
            "ASTRA_EMU_MUSICA_MOVIE_READ",
            format!("{kind:?}, OS error {os_error:?}"),
        ),
    }
}
fn copy_movie(
    mut source: impl Read,
    output: &mut dyn Write,
    expected: u64,
    stop: &AtomicBool,
) -> Result<(), PazError> {
    let mut buffer = [0; 64 * 1024];
    let mut written = 0u64;
    while written < expected {
        if stop.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let count = (expected - written).min(buffer.len() as u64) as usize;
        let read = source.read(&mut buffer[..count]).map_err(stream_error)?;
        if read == 0 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                "movie ended before its declared size",
            ));
        }
        output.write_all(&buffer[..read]).map_err(stream_error)?;
        written += read as u64;
    }
    // Preserve the existing entry reader's allowance for at most 16 zero bytes.
    let mut padding = 0;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let read = source
            .read(&mut buffer[..17 - padding])
            .map_err(stream_error)?;
        if read == 0 {
            break;
        }
        padding += read;
        if padding > 16 || buffer[..read].iter().any(|byte| *byte != 0) {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                "movie exceeds its declared size",
            ));
        }
    }
    output.flush().map_err(stream_error)
}
impl MusicaMountedVfs {
    pub(crate) fn write_movie(
        &self,
        uri: &str,
        output: &mut dyn Write,
        limit: u64,
        stop: &AtomicBool,
    ) -> Result<(), PazError> {
        if stop.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let entry = self.entry(uri)?;
        let descriptor = &entry.descriptor;
        if descriptor.archive_role != "mov"
            || descriptor.unpacked_size == 0
            || descriptor.unpacked_size > limit
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_MOVIE_RESOURCE",
                "movie role or encoded byte budget is invalid",
            ));
        }
        let scheme = self.decrypt_provider.scheme(&descriptor.archive_role)?;
        let archive = &self.archives[entry.archive];
        verify_source_unchanged(archive)?;
        if descriptor
            .offset
            .checked_add(descriptor.stored_size)
            .is_none_or(|end| end > archive.length)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_BOUNDS",
                "movie range exceeds archive parts",
            ));
        }
        let reader = MovieReader {
            archive,
            part: 0,
            part_offset: descriptor.offset,
            remaining: descriptor.stored_size,
            position: 0,
            file: None,
            cipher: MovieCipher::new(
                archive.version,
                descriptor,
                archive.xor_key,
                password_for_entry(descriptor, scheme).unwrap_or_default(),
            )?,
            stop,
        };
        if descriptor.packed {
            copy_movie(
                ZlibDecoder::new(reader),
                output,
                descriptor.unpacked_size,
                stop,
            )?;
        } else {
            copy_movie(reader, output, descriptor.unpacked_size, stop)?;
        }
        verify_source_unchanged(archive)
    }
}
