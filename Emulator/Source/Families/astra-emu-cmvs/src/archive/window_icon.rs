use super::*;
use crate::CmvsStringSegment;
use std::io::Cursor;

const MAX_ICON_FILE: u64 = 16 * 1024 * 1024;

fn failure() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_WINDOW_ICON",
        "native window icon path, format or resource bound is unsupported",
    )
}

impl CmvsArchive {
    pub(crate) fn load_window_icon(
        &self,
        segments: &[CmvsStringSegment],
    ) -> Result<Option<image::RgbaImage>, CoreError> {
        let [CmvsStringSegment::InterpreterPrefixField {
            field_offset: 10764,
        }, CmvsStringSegment::OwnedString(relative)] = segments
        else {
            return Err(failure());
        };
        native::validate_search_path(relative)?;
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        let relative = relative.replace('\\', "/");
        let metadata = match std::fs::metadata(root.join(&relative)) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                return Ok(None)
            }
            Err(_) => return Err(failure()),
        };
        if metadata.len() > MAX_ICON_FILE {
            return Err(failure());
        }
        let path = astra_emu_sdk::resolve_game_file(root, &relative)?;
        let bytes = native::read_native_range(&path, 0, metadata.len())?;
        decode_window_icon(&bytes)
    }
}

fn decode_window_icon(bytes: &[u8]) -> Result<Option<image::RgbaImage>, CoreError> {
    if bytes.len() as u64 > MAX_ICON_FILE {
        return Err(failure());
    }
    // LoadImageA(IMAGE_ICON, 0, 0, LR_LOADFROMFILE) selects the first
    // resource size. image's generic ICO decoder instead picks the largest
    // entry, so expose exactly the native entry without decoding pixels here.
    if bytes.len() < 22 || bytes[..4] != [0, 0, 1, 0] {
        return Ok(None);
    }
    let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
    let directory_end = 6 + count * 16;
    if count == 0 || directory_end > bytes.len() {
        return Ok(None);
    }
    let entry = &bytes[6..22];
    let dimension = |value: u8| if value == 0 { 256 } else { u32::from(value) };
    let expected = (dimension(entry[0]), dimension(entry[1]));
    let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
    let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
    let Some(end) = offset.checked_add(length) else {
        return Ok(None);
    };
    if length == 0 || offset < directory_end || end > bytes.len() {
        return Ok(None);
    }
    let mut single = Vec::with_capacity(22 + length);
    single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    single.extend_from_slice(entry);
    single[18..22].copy_from_slice(&22u32.to_le_bytes());
    single.extend_from_slice(&bytes[offset..end]);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(4 * 1024 * 1024);
    let mut reader = image::ImageReader::with_format(Cursor::new(single), image::ImageFormat::Ico);
    reader.limits(limits);
    match reader.decode() {
        Ok(image) if (image.width(), image.height()) == expected => Ok(Some(image.into_rgba8())),
        Ok(_) | Err(image::ImageError::Decoding(_)) => Ok(None),
        Err(_) => Err(failure()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::ico::{IcoEncoder, IcoFrame};

    fn icon() -> Vec<u8> {
        let first = vec![255; 4 * 16 * 16];
        let second = vec![127; 4 * 64 * 64];
        let frames = [
            IcoFrame::as_png(&first, 16, 16, image::ExtendedColorType::Rgba8).unwrap(),
            IcoFrame::as_png(&second, 64, 64, image::ExtendedColorType::Rgba8).unwrap(),
        ];
        let mut bytes = Vec::new();
        IcoEncoder::new(&mut bytes).encode_images(&frames).unwrap();
        bytes
    }

    #[test]
    fn selects_first_resource_instead_of_largest_and_checks_allocation_bounds() {
        let bytes = icon();
        let image = decode_window_icon(&bytes).unwrap().unwrap();
        assert_eq!(image.dimensions(), (16, 16));
        assert!(image.as_raw().iter().all(|byte| *byte == 255));
        for range in [4..6, 14..18, 18..22] {
            let mut corrupt = bytes.clone();
            corrupt[range].fill(255);
            assert!(decode_window_icon(&corrupt).unwrap().is_none());
        }
        let mut mismatch = bytes.clone();
        mismatch[6] = 32;
        assert!(decode_window_icon(&mismatch).unwrap().is_none());
        assert!(decode_window_icon(&[0; 21]).unwrap().is_none());
    }

    #[test]
    fn missing_icon_is_native_best_effort_but_unsafe_paths_fail() {
        let (root, mut archive) = super::super::cache_tests::fixture();
        archive.bind_game_root(root.path().to_path_buf()).unwrap();
        let request = |name: &str| {
            vec![
                CmvsStringSegment::InterpreterPrefixField {
                    field_offset: 10764,
                },
                CmvsStringSegment::OwnedString(name.into()),
            ]
        };
        assert!(archive
            .load_window_icon(&request("absent.ico"))
            .unwrap()
            .is_none());
        assert!(archive.load_window_icon(&request("../escape.ico")).is_err());
        let bytes = icon();
        std::fs::write(root.path().join("game.ico"), &bytes).unwrap();
        assert_eq!(
            archive
                .load_window_icon(&request("game.ico"))
                .unwrap()
                .unwrap()
                .dimensions(),
            (16, 16)
        );
        assert_eq!(std::fs::read(root.path().join("game.ico")).unwrap(), bytes);
    }
}
