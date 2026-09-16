use astra_media_core::{MediaError, TextureFrame};
use lru::LruCache;
use std::{io::Cursor, num::NonZeroUsize};

/// CPU asset residency bounded by both entry count and total decoded RGBA bytes.
///
/// Families retain their own archive lookup and format decoders. Common image
/// formats use `decode`; proprietary decoders may insert a validated frame.
/// Cloned frames may outlive eviction, so this limit describes cache residency,
/// not all resources retained by a renderer or the caller.
pub struct TextureCache {
    entries: LruCache<String, TextureFrame>,
    bytes: usize,
    max_bytes: usize,
    max_dimension: u32,
}

impl TextureCache {
    pub fn new(
        max_entries: NonZeroUsize,
        max_bytes: usize,
        max_dimension: u32,
    ) -> Result<Self, MediaError> {
        if max_bytes == 0 || max_dimension == 0 {
            return Err(MediaError::message("ASTRA_EMU_SDK_TEXTURE_BUDGET"));
        }
        Ok(Self {
            entries: LruCache::new(max_entries),
            bytes: 0,
            max_bytes,
            max_dimension,
        })
    }

    pub fn get(&mut self, id: &str) -> Option<TextureFrame> {
        self.entries.get(id).cloned()
    }

    pub fn resident_bytes(&self) -> usize {
        self.bytes
    }

    pub fn insert(&mut self, id: String, frame: TextureFrame) -> Result<(), MediaError> {
        let expected = self.byte_count(frame.width, frame.height)?;
        if id.is_empty() || frame.rgba8.len() != expected {
            return Err(MediaError::message("ASTRA_EMU_SDK_TEXTURE_FRAME"));
        }
        if let Some(previous) = self.entries.pop(&id) {
            self.bytes -= previous.rgba8.len();
        }
        while self.bytes > self.max_bytes - expected {
            let (_, evicted) = self
                .entries
                .pop_lru()
                .ok_or_else(|| MediaError::message("ASTRA_EMU_SDK_TEXTURE_CACHE_ACCOUNTING"))?;
            self.bytes -= evicted.rgba8.len();
        }
        if let Some((_, evicted)) = self.entries.push(id, frame) {
            self.bytes -= evicted.rgba8.len();
        }
        self.bytes += expected;
        Ok(())
    }

    /// Decode a standard image through the existing image crate with allocation
    /// and dimensions checked before RGBA conversion can allocate a larger buffer.
    pub fn decode(&mut self, id: String, bytes: &[u8]) -> Result<TextureFrame, MediaError> {
        let dimensions = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| MediaError::message("ASTRA_EMU_SDK_IMAGE_FORMAT"))?
            .into_dimensions()
            .map_err(|_| MediaError::message("ASTRA_EMU_SDK_IMAGE_DIMENSIONS"))?;
        self.byte_count(dimensions.0, dimensions.1)?;
        let mut reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| MediaError::message("ASTRA_EMU_SDK_IMAGE_FORMAT"))?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.max_dimension);
        limits.max_image_height = Some(self.max_dimension);
        limits.max_alloc = Some(self.max_bytes as u64);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|_| MediaError::message("ASTRA_EMU_SDK_IMAGE_DECODE"))?;
        self.byte_count(decoded.width(), decoded.height())?;
        let rgba = decoded.to_rgba8();
        let frame = TextureFrame {
            width: rgba.width(),
            height: rgba.height(),
            rgba8: rgba.into_raw().into(),
        };
        self.insert(id, frame.clone())?;
        Ok(frame)
    }

    fn byte_count(&self, width: u32, height: u32) -> Result<usize, MediaError> {
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4));
        if width == 0
            || height == 0
            || width > self.max_dimension
            || height > self.max_dimension
            || bytes.is_none_or(|bytes| bytes > self.max_bytes)
        {
            return Err(MediaError::message("ASTRA_EMU_SDK_TEXTURE_BOUND"));
        }
        Ok(bytes.expect("validated byte count"))
    }
}
