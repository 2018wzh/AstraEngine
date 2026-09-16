use astra_emu_sdk::TextureCache;
use astra_media_core::TextureFrame;
use std::{io::Cursor, num::NonZeroUsize};

fn frame(value: u8) -> TextureFrame {
    TextureFrame {
        width: 2,
        height: 2,
        rgba8: vec![value; 16].into(),
    }
}

#[test]
fn byte_and_entry_limits_respect_recent_use_and_replacement() {
    let mut cache = TextureCache::new(NonZeroUsize::new(2).unwrap(), 32, 8).unwrap();
    cache.insert("a".into(), frame(1)).unwrap();
    cache.insert("b".into(), frame(2)).unwrap();
    let retained = cache.get("a").unwrap();
    cache.insert("c".into(), frame(3)).unwrap();
    assert!(cache.get("b").is_none());
    assert_eq!(cache.resident_bytes(), 32);
    cache.insert("a".into(), frame(4)).unwrap();
    assert_eq!(cache.resident_bytes(), 32);
    assert_eq!(retained.rgba8[0], 1);
    assert_eq!(cache.get("a").unwrap().rgba8[0], 4);
    let invalid = TextureFrame {
        width: 3,
        height: 3,
        rgba8: vec![0; 36].into(),
    };
    assert!(cache.insert("a".into(), invalid).is_err());
    assert_eq!(cache.get("a").unwrap().rgba8[0], 4);
}

#[test]
fn image_decode_rejects_corruption_and_rgba_expansion_before_replacing_cache() {
    let mut cache = TextureCache::new(NonZeroUsize::new(4).unwrap(), 16, 8).unwrap();
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let decoded = cache.decode("image".into(), encoded.get_ref()).unwrap();
    assert_eq!(decoded.rgba8.len(), 16);
    assert!(cache.decode("image".into(), b"not an image").is_err());
    assert_eq!(cache.resident_bytes(), 16);
    let mut oversized = Cursor::new(Vec::new());
    image::DynamicImage::new_luma8(3, 3)
        .write_to(&mut oversized, image::ImageFormat::Png)
        .unwrap();
    assert!(cache.decode("image".into(), oversized.get_ref()).is_err());
    assert_eq!(cache.get("image").unwrap().width, 2);
}
