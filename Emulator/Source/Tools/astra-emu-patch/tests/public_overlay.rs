mod fixtures;
use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use fixtures::PublicOverlay;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};

fn request(fixture: &PublicOverlay) -> PatchRequest {
    PatchRequest {
        script: fixture.script.clone(),
        source: fixture.source.clone(),
        patch: fixture.patch.clone(),
        output: fixture.output.clone(),
    }
}

#[test]
fn pe_overlay_patches_both_text_slots_and_images_without_changing_originals() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    let patch_before = fs::read(fixture.patch.join("overlay.exe")).unwrap();
    let result = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(
        fs::read(result.output.join("assets.bin")).unwrap(),
        fixture.expected
    );
    assert_eq!(
        fs::read(result.output.join("unchanged.dat")).unwrap(),
        b"complete independent public fixture"
    );
    assert_eq!(
        fs::metadata(result.output.join("zero.dat")).unwrap().len(),
        0
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("astra-patch-result.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["recipe"]["id"], "public-pe-overlay");
    assert_eq!(
        receipt["profile"]["configuration"]["script_encoding"],
        "utf8"
    );
    fixture.assert_inputs();
    assert_eq!(
        fs::read(fixture.patch.join("overlay.exe")).unwrap(),
        patch_before
    );
}

#[test]
fn independent_crc_mismatch_is_integrity_failure_and_never_publishes() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = PublicOverlay::new(root.path());
    fixture.blocks[1].2 ^= 1;
    fixture.write_patch();
    fixture.write_script();
    let error = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .unwrap();
    assert_eq!(error.kind(), PatchErrorKind::Integrity);
    assert!(!fixture.output.exists());
    fixture.assert_inputs();
}

#[test]
fn truncated_zlib_payload_fails_at_decoder_boundary() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = PublicOverlay::new(root.path());
    let new_length = fixture.blocks[2].3.len() - 2;
    fixture.blocks[2].3.truncate(new_length);
    fixture.write_patch();
    fixture.write_script();
    let error = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .unwrap();
    assert_eq!(error.kind(), PatchErrorKind::Codec);
    assert!(!fixture.output.exists());
    fixture.assert_inputs();
}

#[test]
fn declared_inflate_length_is_not_treated_as_a_hint() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = PublicOverlay::new(root.path());
    fixture.blocks[0].1 -= 1;
    fixture.write_patch();
    fixture.write_script();
    let error = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .unwrap();
    assert_eq!(error.kind(), PatchErrorKind::Codec);
    assert!(!fixture.output.exists());
    fixture.assert_inputs();
}
