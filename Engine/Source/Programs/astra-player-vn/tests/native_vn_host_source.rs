use astra_core::Hash256;
use astra_player_core::{PlayerAction, PlayerHostCommand, PlayerHostResourceId};
use astra_player_vn::NativeVnHostCommandSource;
use astra_vn_core::{compile_astra_sources, AstraSource, VnRunConfig};

const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one
    text key:line.two speaker:hero #@id line.two
"#;

#[test]
fn native_vn_source_turns_real_runtime_steps_into_changing_frames() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    let first = source.launch().unwrap();
    let second = source.dispatch_action(PlayerAction::Advance).unwrap();
    let frame_hash = |command: &PlayerHostCommand| match command {
        PlayerHostCommand::PresentRgba { rgba8, .. } => Hash256::from_sha256(rgba8),
        _ => panic!("expected present command"),
    };
    assert_ne!(
        frame_hash(&first.commands[0]),
        frame_hash(&second.commands[0])
    );
}
