use std::collections::BTreeMap;

use astra_core::Hash256;
use astra_media::{
    AudioCommand, PcmAsset, PcmAssetResolver, ProductionAudioMixer, CANONICAL_FRAMES_PER_TICK,
};

struct Resolver(BTreeMap<String, PcmAsset>);
impl PcmAssetResolver for Resolver {
    fn resolve_canonical(&self, asset: &str) -> Result<PcmAsset, astra_media::MediaError> {
        self.0
            .get(asset)
            .cloned()
            .ok_or_else(|| astra_media::MediaError::message("missing asset"))
    }
}

fn asset(id: &str, frames: usize, value: f32) -> PcmAsset {
    let samples = vec![value; frames * 2];
    let bytes = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect::<Vec<_>>();
    PcmAsset::new(id, Hash256::from_sha256(&bytes), samples).unwrap()
}

#[astra_headless_test::test]
fn mixes_exact_tick_with_loop_seek_fade_and_snapshot_restore() {
    let resolver = Resolver(BTreeMap::from([
        ("asset:/voice".into(), asset("asset:/voice", 1_600, 0.25)),
        ("asset:/loop".into(), asset("asset:/loop", 400, 0.1)),
    ]));
    let mut mixer = ProductionAudioMixer::new(8).unwrap();
    mixer
        .apply(
            AudioCommand::play_voice("voice-a", "voice", "asset:/voice", 34, false),
            &resolver,
        )
        .unwrap();
    mixer
        .apply(
            AudioCommand::play_voice("loop-a", "music", "asset:/loop", 8, true),
            &resolver,
        )
        .unwrap();
    mixer
        .apply(AudioCommand::fade_bus("fade", "music", 0.0, 16), &resolver)
        .unwrap();
    let first = mixer.render_tick().unwrap();
    assert_eq!(first.samples.len(), CANONICAL_FRAMES_PER_TICK * 2);
    assert!(first.peak_dbfs.is_finite());
    assert_eq!(first.completed_fades, ["fade"]);
    assert_eq!(mixer.voice_bus("loop-a"), Some("music"));
    let snapshot = mixer.snapshot();
    let mut restored = ProductionAudioMixer::restore(snapshot, &resolver, 8).unwrap();
    let second = restored.render_tick().unwrap();
    assert!(second.completed_voices.contains(&"voice-a".to_string()));
    assert_eq!(restored.snapshot().voices.len(), 1);
}

#[astra_headless_test::test]
fn restore_revalidates_asset_hash() {
    let resolver = Resolver(BTreeMap::from([(
        "asset:/voice".into(),
        asset("asset:/voice", 1_600, 0.25),
    )]));
    let mut mixer = ProductionAudioMixer::new(1).unwrap();
    mixer
        .apply(
            AudioCommand::play_voice("voice-a", "voice", "asset:/voice", 34, false),
            &resolver,
        )
        .unwrap();
    let snapshot = mixer.snapshot();
    let changed = Resolver(BTreeMap::from([(
        "asset:/voice".into(),
        asset("asset:/voice", 1_600, 0.5),
    )]));
    assert!(ProductionAudioMixer::restore(snapshot, &changed, 1).is_err());
}
