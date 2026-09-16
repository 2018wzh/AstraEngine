use super::*;

#[test]
fn native_stop_tokens_are_not_missing_archive_resources() {
    let script = parse_sc(
        b".playbgm *\r\n.playse *\r\n.playse2 *\r\n.playse3 *\r\n.playvoice *\r\n.playbgm absent.ogg\r\n.playbgm -\r\n",
        &ScOpcodeCatalog::observed_minori(),
    )
    .unwrap();
    let hash = Hash256::from_sha256(b"census-test");
    let manifest = astra_emu_minori::ArchiveManifest {
        schema: "astra.emu.archive_manifest.v1".into(),
        family_id: "minori".into(),
        mount_id: "test".into(),
        prefix: "minori:/".into(),
        reader_id: "test".into(),
        reader_hash: hash,
        decrypt_provider_id: "test".into(),
        private_profile_hash: hash,
        mount_profile_hash: hash,
        sources: Vec::new(),
        entries: Vec::new(),
    };
    let census = census_audio_resources(&[script], &manifest).unwrap();
    assert_eq!(census.reference_count, 7);
    assert_eq!(census.stop_token_count, 5);
    assert_eq!(census.candidate_missing_count, 2);
    assert_eq!(census.malformed_count, 0);
}
