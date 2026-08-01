use std::{io::Read, sync::Arc};

use astra_emu_family_core::LegacyCoreError;
use encoding_rs::SHIFT_JIS;
use flate2::read::ZlibDecoder;
use image::RgbaImage;

const MAX_CONTAINER_BYTES: usize = 1024 * 1024 * 1024;
const MAX_FRAME_COUNT: usize = 65_536;
const MAX_NAME_BYTES: usize = 4_096;
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriImageFrameDescriptor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub offset_x: i16,
    pub offset_y: i16,
    pub bits_per_pixel: u16,
    pub data_offset: u64,
    pub data_size: u64,
}

#[derive(Debug, Clone)]
pub struct MinoriAniArchive {
    source: Arc<[u8]>,
    frames: Vec<MinoriImageFrameDescriptor>,
}

impl MinoriAniArchive {
    pub fn parse(source: impl Into<Arc<[u8]>>) -> Result<Self, LegacyCoreError> {
        let source = source.into();
        validate_container_size(source.len())?;
        if read_u16(&source, 0)? != 0x0100 || read_u32(&source, 4)? != 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_ANI_HEADER",
                "ANI header is invalid",
            ));
        }
        let count = read_i16(&source, 2)?;
        if count <= 0 || count as usize > MAX_FRAME_COUNT {
            return Err(invalid(
                "ASTRA_EMU_MINORI_ANI_FRAME_COUNT",
                "ANI frame count is invalid",
            ));
        }
        let mut cursor = 8usize;
        let mut frames = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name_end = source[cursor..]
                .iter()
                .take(MAX_NAME_BYTES + 1)
                .position(|byte| *byte == 0)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_ANI_NAME",
                        "ANI frame name is not bounded or terminated",
                    )
                })?;
            if name_end == 0 {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_ANI_NAME",
                    "ANI frame name is empty",
                ));
            }
            let name_bytes = checked_slice(&source, cursor, name_end)?;
            let (name, _, had_errors) = SHIFT_JIS.decode(name_bytes);
            if had_errors || name.trim().is_empty() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_ANI_NAME",
                    "ANI frame name is not valid CP932",
                ));
            }
            cursor = cursor
                .checked_add(name_end + 1)
                .ok_or_else(|| invalid("ASTRA_EMU_MINORI_ANI_BOUNDS", "ANI cursor overflowed"))?;
            let width = u32::from(read_u16(&source, cursor)?);
            let height = u32::from(read_u16(&source, cursor + 2)?);
            let bits_per_pixel = read_u16(&source, cursor + 4)?;
            let offset_x = read_i16(&source, cursor + 6)?;
            let offset_y = read_i16(&source, cursor + 8)?;
            let pixel_bytes = checked_pixel_bytes(width, height, bits_per_pixel)?;
            let data_offset = cursor.checked_add(10).ok_or_else(|| {
                invalid("ASTRA_EMU_MINORI_ANI_BOUNDS", "ANI data offset overflowed")
            })?;
            checked_slice(&source, data_offset, pixel_bytes)?;
            frames.push(MinoriImageFrameDescriptor {
                name: name.into_owned(),
                width,
                height,
                offset_x,
                offset_y,
                bits_per_pixel,
                data_offset: data_offset as u64,
                data_size: pixel_bytes as u64,
            });
            cursor = data_offset + pixel_bytes;
        }
        if cursor != source.len() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_ANI_TRAILING_DATA",
                "ANI contains trailing data",
            ));
        }
        Ok(Self { source, frames })
    }

    pub fn frames(&self) -> &[MinoriImageFrameDescriptor] {
        &self.frames
    }

    pub fn decode_frame(&self, index: usize) -> Result<RgbaImage, LegacyCoreError> {
        let frame = self.frames.get(index).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_ANI_FRAME_INDEX",
                "ANI frame index is outside the archive",
            )
        })?;
        let pixels = checked_slice(
            &self.source,
            frame.data_offset as usize,
            frame.data_size as usize,
        )?;
        raw_to_rgba(frame.width, frame.height, frame.bits_per_pixel, pixels)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriSqzFrameDescriptor {
    pub index: u32,
    pub data_offset: u64,
    pub stored_size: u64,
}

#[derive(Debug, Clone)]
pub struct MinoriSqzArchive {
    source: Arc<[u8]>,
    width: u32,
    height: u32,
    frames: Vec<MinoriSqzFrameDescriptor>,
}

impl MinoriSqzArchive {
    pub fn parse(source: impl Into<Arc<[u8]>>) -> Result<Self, LegacyCoreError> {
        let source = source.into();
        validate_container_size(source.len())?;
        if checked_slice(&source, 0, 4)? != b"SQZ1" {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SQZ_HEADER",
                "SQZ header is invalid",
            ));
        }
        let width = read_u32(&source, 8)?;
        let height = read_u32(&source, 12)?;
        checked_pixel_bytes(width, height, 32)?;
        let pair_count = read_u32(&source, 16)? as usize;
        let count = pair_count
            .checked_mul(2)
            .filter(|count| *count > 0 && *count <= MAX_FRAME_COUNT)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SQZ_FRAME_COUNT",
                    "SQZ frame count is invalid",
                )
            })?;
        let index_end = 0x14usize
            .checked_add(count.checked_mul(8).ok_or_else(|| {
                invalid("ASTRA_EMU_MINORI_SQZ_INDEX", "SQZ index size overflowed")
            })?)
            .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SQZ_INDEX", "SQZ index end overflowed"))?;
        checked_slice(&source, 0, index_end)?;
        let mut frames = Vec::with_capacity(count);
        for index in 0..count {
            let cursor = 0x14 + index * 8;
            let data_offset = read_u32(&source, cursor)? as usize;
            let stored_size = read_u32(&source, cursor + 4)? as usize;
            if data_offset < index_end || stored_size == 0 {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SQZ_ENTRY",
                    "SQZ frame range overlaps metadata or is empty",
                ));
            }
            checked_slice(&source, data_offset, stored_size)?;
            frames.push(MinoriSqzFrameDescriptor {
                index: index as u32,
                data_offset: data_offset as u64,
                stored_size: stored_size as u64,
            });
        }
        Ok(Self {
            source,
            width,
            height,
            frames,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn frames(&self) -> &[MinoriSqzFrameDescriptor] {
        &self.frames
    }

    pub fn decode_frame(&self, index: usize) -> Result<RgbaImage, LegacyCoreError> {
        let frame = self.frames.get(index).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SQZ_FRAME_INDEX",
                "SQZ frame index is outside the archive",
            )
        })?;
        let encoded = checked_slice(
            &self.source,
            frame.data_offset as usize,
            frame.stored_size as usize,
        )?;
        let expected = checked_pixel_bytes(self.width, self.height, 32)?;
        let limit = expected
            .checked_add(1)
            .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SQZ_OUTPUT", "SQZ output bound overflowed"))?;
        let mut bgra = Vec::with_capacity(expected);
        ZlibDecoder::new(encoded)
            .take(limit as u64)
            .read_to_end(&mut bgra)
            .map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SQZ_ZLIB",
                    "SQZ frame zlib stream is invalid",
                )
            })?;
        if bgra.len() != expected {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SQZ_OUTPUT",
                "SQZ frame output size is invalid",
            ));
        }
        raw_to_rgba(self.width, self.height, 32, &bgra)
    }
}

fn raw_to_rgba(
    width: u32,
    height: u32,
    bits_per_pixel: u16,
    source: &[u8],
) -> Result<RgbaImage, LegacyCoreError> {
    let expected = checked_pixel_bytes(width, height, bits_per_pixel)?;
    if source.len() != expected {
        return Err(invalid(
            "ASTRA_EMU_MINORI_IMAGE_SIZE",
            "raw image size does not match its descriptor",
        ));
    }
    let pixel_count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_IMAGE_SIZE",
            "raw image pixel count overflowed",
        )
    })?;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    match bits_per_pixel {
        32 => source
            .chunks_exact(4)
            .for_each(|pixel| rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]])),
        24 => source
            .chunks_exact(3)
            .for_each(|pixel| rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255])),
        16 => source.chunks_exact(2).for_each(|pixel| {
            let value = u16::from_le_bytes([pixel[0], pixel[1]]);
            let r = ((value >> 11) & 0x1f) as u8;
            let g = ((value >> 5) & 0x3f) as u8;
            let b = (value & 0x1f) as u8;
            rgba.extend_from_slice(&[
                (r << 3) | (r >> 2),
                (g << 2) | (g >> 4),
                (b << 3) | (b >> 2),
                255,
            ]);
        }),
        8 => source
            .iter()
            .for_each(|value| rgba.extend_from_slice(&[*value, *value, *value, 255])),
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_IMAGE_BPP",
                "raw image bit depth is unsupported",
            ))
        }
    }
    RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_IMAGE_SIZE",
            "RGBA image allocation is invalid",
        )
    })
}

fn checked_pixel_bytes(
    width: u32,
    height: u32,
    bits_per_pixel: u16,
) -> Result<usize, LegacyCoreError> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || !matches!(bits_per_pixel, 8 | 16 | 24 | 32)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_IMAGE_DESCRIPTOR",
            "image dimensions or bit depth are invalid",
        ));
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .filter(|pixels| *pixels <= MAX_PIXELS)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_IMAGE_SIZE",
                "image pixel budget is exceeded",
            )
        })?;
    usize::try_from(pixels * u64::from(bits_per_pixel / 8))
        .map_err(|_| invalid("ASTRA_EMU_MINORI_IMAGE_SIZE", "image byte size overflowed"))
}

fn validate_container_size(len: usize) -> Result<(), LegacyCoreError> {
    if len == 0 || len > MAX_CONTAINER_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_IMAGE_CONTAINER_SIZE",
            "image container size is invalid",
        ));
    }
    Ok(())
}

fn checked_slice(source: &[u8], offset: usize, len: usize) -> Result<&[u8], LegacyCoreError> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_IMAGE_BOUNDS", "image range overflowed"))?;
    source
        .get(offset..end)
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_IMAGE_BOUNDS", "image range is truncated"))
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, LegacyCoreError> {
    Ok(u16::from_le_bytes(
        checked_slice(source, offset, 2)?.try_into().unwrap(),
    ))
}
fn read_i16(source: &[u8], offset: usize) -> Result<i16, LegacyCoreError> {
    Ok(i16::from_le_bytes(
        checked_slice(source, offset, 2)?.try_into().unwrap(),
    ))
}
fn read_u32(source: &[u8], offset: usize) -> Result<u32, LegacyCoreError> {
    Ok(u32::from_le_bytes(
        checked_slice(source, offset, 4)?.try_into().unwrap(),
    ))
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{write::ZlibEncoder, Compression};

    use super::*;

    #[test]
    fn ani_decodes_all_observed_pixel_formats() {
        for (bpp, raw, expected) in [
            (32, vec![1, 2, 3, 4], [3, 2, 1, 4]),
            (24, vec![1, 2, 3], [3, 2, 1, 255]),
            (16, 0xf800u16.to_le_bytes().to_vec(), [255, 0, 0, 255]),
            (8, vec![7], [7, 7, 7, 255]),
        ] {
            let mut bytes = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0, b'f', 0];
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&(bpp as u16).to_le_bytes());
            bytes.extend_from_slice(&(-2i16).to_le_bytes());
            bytes.extend_from_slice(&3i16.to_le_bytes());
            bytes.extend_from_slice(&raw);
            let archive = MinoriAniArchive::parse(Arc::<[u8]>::from(bytes)).unwrap();
            assert_eq!(
                (archive.frames()[0].offset_x, archive.frames()[0].offset_y),
                (-2, 3)
            );
            assert_eq!(archive.decode_frame(0).unwrap().as_raw(), &expected);
        }
    }

    #[test]
    fn ani_rejects_truncation_and_trailing_data() {
        let bytes = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0, b'f', 0];
        assert_eq!(
            MinoriAniArchive::parse(Arc::<[u8]>::from(bytes))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_IMAGE_BOUNDS"
        );
    }

    #[test]
    fn sqz_decodes_bgra_and_enforces_exact_output() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&[1, 2, 3, 4]).unwrap();
        let frame = encoder.finish().unwrap();
        let index_end = 0x24u32;
        let mut bytes = b"SQZ1".to_vec();
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for _ in 0..2 {
            bytes.extend_from_slice(&index_end.to_le_bytes());
            bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&frame);
        let archive = MinoriSqzArchive::parse(Arc::<[u8]>::from(bytes)).unwrap();
        assert_eq!(archive.frames().len(), 2);
        assert_eq!(archive.decode_frame(1).unwrap().as_raw(), &[3, 2, 1, 4]);
    }

    #[test]
    fn sqz_rejects_metadata_overlap_and_output_overrun() {
        let mut bytes = vec![0; 0x24];
        bytes[..4].copy_from_slice(b"SQZ1");
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            MinoriSqzArchive::parse(Arc::<[u8]>::from(bytes))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_SQZ_ENTRY"
        );
    }
}
