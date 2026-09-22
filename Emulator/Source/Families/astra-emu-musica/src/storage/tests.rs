use super::*;
use astra_emu_sdk::StageCanvas;
use astra_media_core::Extent2D;

fn snapshot(game: &[u8], message: &str) -> Snapshot {
    Snapshot {
        logical_extent: [1280, 720],
        card: SaveCard::capture(1280, 720, 1280, 720, &vec![255; 1280 * 720 * 4]).unwrap(),
        game: Hash256::from_sha256(game),
        vm: vec![],
        message: Some((message.into(), None)),
        wait_ns: 0,
        sounds: vec![],
    }
}

#[test]
fn save_cards_accept_arbitrary_raster_extents() {
    for (width, height) in [(1280, 720), (721, 1280), (1279, 719), (641, 359), (17, 1)] {
        let rgba = vec![255; width as usize * height as usize * 4];
        let card = SaveCard::capture(1280, 720, width, height, &rgba).unwrap();
        let thumbnail = card.texture().unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (96, 54));
    }
}

#[test]
fn save_card_crops_content_before_resizing_the_thumbnail() {
    let width = 721_u32;
    let height = 1280_u32;
    let canvas = StageCanvas::new(Extent2D::new(1280, 720), Extent2D::new(width, height)).unwrap();
    let viewport = canvas.viewport();
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for y in viewport.y..viewport.y + viewport.height {
        for x in viewport.x..viewport.x + viewport.width {
            let offset = ((y * width + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 32, 16, 255]);
        }
    }
    let card = SaveCard::capture(1280, 720, width, height, &rgba).unwrap();
    let thumbnail = card.texture().unwrap();
    assert!(thumbnail
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [255, 32, 16, 255]));
}

#[test]
fn save_cards_accept_supported_rasters_and_reject_invalid_frames() {
    for (width, height) in [(1280, 720), (721, 1280), (1279, 719), (641, 359), (17, 1)] {
        let rgba = vec![255; width as usize * height as usize * 4];
        let card = SaveCard::capture(1280, 720, width, height, &rgba).unwrap();
        let thumbnail = card.texture().unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (96, 54));
        assert_eq!(thumbnail.rgba8.len(), 96 * 54 * 4);
    }

    for (width, height, rgba) in [
        (0, 720, vec![]),
        (720, 0, vec![]),
        (1920, 1080, vec![255; 1920 * 1080 * 4 - 1]),
    ] {
        let error = match SaveCard::capture(1280, 720, width, height, &rgba) {
            Ok(_) => panic!("invalid save card frame was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "ASTRA_EMU_MUSICA_SAVE_THUMBNAIL");
    }
}

#[test]
fn arbitrary_size_save_cards_round_trip_through_storage() {
    let root = tempfile::tempdir().unwrap();
    let game = Hash256::from_sha256(b"high-scale-game");
    let storage = Storage::new(root.path()).unwrap();
    for (slot, (width, height)) in [
        (0, (1280, 720)),
        (1, (721, 1280)),
        (2, (1279, 719)),
        (3, (17, 1)),
    ] {
        let rgba = vec![255; width as usize * height as usize * 4];
        let mut card = SaveCard::capture(1280, 720, width, height, &rgba).unwrap();
        card.comment = format!("scale-{width}x{height}");
        let snapshot = Snapshot {
            logical_extent: [1280, 720],
            card,
            game,
            vm: vec![slot as u8],
            message: None,
            wait_ns: 0,
            sounds: vec![],
        };
        storage.write(slot, &snapshot).unwrap();
    }

    let reopened = Storage::new(root.path()).unwrap();
    for (slot, (width, height)) in [
        (0, (1280, 720)),
        (1, (721, 1280)),
        (2, (1279, 719)),
        (3, (17, 1)),
    ] {
        let snapshot = reopened.read(slot).unwrap();
        let thumbnail = snapshot.card.texture().unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (96, 54));
        assert_eq!(snapshot.card.comment, format!("scale-{width}x{height}"));
        assert_eq!(snapshot.vm, vec![slot as u8]);
        assert_eq!(snapshot.game, game);
    }
}

#[test]
fn native_slots_are_independent_and_out_of_range_does_not_create_files() {
    let root = tempfile::tempdir().unwrap();
    let storage = Storage::new(root.path()).unwrap();
    assert_eq!(
        storage
            .write(100, &snapshot(b"game", "bad"))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_SAVE_SLOT"
    );
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    for slot in [0, 10, 19, 20, 99] {
        storage
            .write(slot, &snapshot(b"game", &slot.to_string()))
            .unwrap();
    }
    let reopened = Storage::new(root.path()).unwrap();
    for slot in [0, 10, 19, 20, 99] {
        assert_eq!(
            reopened.read(slot).unwrap().message.unwrap().0,
            slot.to_string()
        );
    }
    assert!(reopened.read(100).is_err());
}

#[test]
fn foreign_and_damaged_manual_slots_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let storage = Storage::new(root.path()).unwrap();
    storage.write(20, &snapshot(b"first", "original")).unwrap();
    let path = storage.path(20, false).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert_eq!(
        storage
            .write(20, &snapshot(b"second", "replacement"))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_SAVE_GAME"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::write(&path, b"damaged save").unwrap();
    assert!(storage
        .write(20, &snapshot(b"first", "replacement"))
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"damaged save");
    storage
        .write(21, &snapshot(b"first", "independent"))
        .unwrap();
}

#[test]
fn malformed_save_cards_and_previous_formats_are_not_replaced() {
    let root = tempfile::tempdir().unwrap();
    let storage = Storage::new(root.path()).unwrap();
    let original = snapshot(b"game", "original");
    storage.write(20, &original).unwrap();
    let path = storage.path(20, false).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    for timestamp in [
        "2026/02/30 12:00",
        "2026/01/01 24:00",
        "0000/01/01 00:00",
        "invalid",
    ] {
        let mut bad = snapshot(b"game", "replacement");
        bad.card.timestamp = timestamp.into();
        assert!(storage.write(20, &bad).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let mut bad = snapshot(b"game", "replacement");
    bad.card.thumbnail_png = vec![0; 32];
    assert!(storage.write(20, &bad).is_err());
    bad.card = original.card.clone();
    bad.card.comment = "invalid\ncomment".into();
    assert!(storage.write(20, &bad).is_err());
    for magic in [b"AMINSV02", b"AMUSSV03", b"AMUSSV04"] {
        let mut old = bytes.clone();
        old[..8].copy_from_slice(magic);
        std::fs::write(&path, &old).unwrap();
        assert!(storage.read(20).is_err());
        assert!(storage.write(20, &original).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
    }
}

#[test]
fn quick_cursor_is_bounded_persistent_and_rejects_foreign_or_corrupt_data() {
    let root = tempfile::tempdir().unwrap();
    let game = Hash256::from_sha256(b"game");
    let storage = Storage::new(root.path()).unwrap();
    assert_eq!(storage.quick_cursor(game).unwrap(), 0);
    storage.write_quick_cursor(game, 9).unwrap();
    let reopened = Storage::new(root.path()).unwrap();
    assert_eq!(reopened.quick_cursor(game).unwrap(), 9);
    let path = storage.named_path("quick-cursor.json", false).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(storage.write_quick_cursor(game, 10).is_err());
    assert!(storage
        .write_quick_cursor(Hash256::from_sha256(b"other"), 0)
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::write(&path, b"corrupt cursor").unwrap();
    assert!(storage.write_quick_cursor(game, 0).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt cursor");
}

#[test]
fn gallery_progress_preserves_damaged_foreign_and_monotonic_state() {
    let root = tempfile::tempdir().unwrap();
    let storage = Storage::new(root.path()).unwrap();
    let game = Hash256::from_sha256(b"game");
    let unlocks = [Hash256::from_sha256(b"REN_CLEAR")];
    assert!(storage.progress(game).unwrap().is_empty());
    storage.write_progress(game, &unlocks).unwrap();
    assert_eq!(storage.progress(game).unwrap(), unlocks);
    assert!(storage.write_progress(game, &[]).is_err());
    assert!(storage
        .write_progress(Hash256::from_sha256(b"other"), &unlocks)
        .is_err());
    assert_eq!(storage.progress(game).unwrap(), unlocks);
    let path = storage.named_path("global-progress.json", false).unwrap();
    fs::write(&path, b"damaged").unwrap();
    assert!(storage.write_progress(game, &unlocks).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"damaged");
}
