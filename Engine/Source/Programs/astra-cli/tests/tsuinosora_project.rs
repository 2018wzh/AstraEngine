use std::{path::Path, process::Command};

use astra_platform::{validate_host_profile, PlatformHostProfile};
use astra_target::TargetManifest;

#[test]
fn generated_stage_commands_compile_with_explicit_replacement_policy() {
    use astra_vn_script::{
        compile_astra_project, AstraSource, CompiledCommand, PresentationCommand,
        PresentationInterruptPolicy, StageCommand,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let generated = Command::new("python")
        .args(["-c", r#"
import json, sys
sys.path.insert(0, 'Tools/TsuiNoSora')
from native_story_ir import _render_command
commands = [
    dict(kind='show', character_id='actor', asset_id='image.actor', layer='main'),
    dict(kind='move', character_id='actor', x=20, y=30, duration_ms=100),
    dict(kind='hide', character_id='actor'),
    dict(kind='background', asset_id='image.room'),
    dict(kind='clear_layer', layer='main'),
    dict(kind='movie', asset_id='video.clip', end='continue'),
]
print(json.dumps([_render_command(dict(command, command_id='command.test'), {})[0] for command in commands]))
"#])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let commands: Vec<String> = serde_json::from_slice(&generated.stdout).unwrap();
    assert_eq!(commands.len(), 6);
    for command in commands {
        let source = AstraSource::story("converted.astra", format!(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n{command}\n"
        ));
        let compiled = compile_astra_project([source], Default::default()).unwrap();
        let CompiledCommand::Presentation {
            command: PresentationCommand::Stage(stage),
            ..
        } = &compiled.states["state.start"].scenes[0].commands[0]
        else {
            panic!("expected a typed stage command");
        };
        let interrupt = match stage {
            StageCommand::Show { interrupt, .. }
            | StageCommand::Move { interrupt, .. }
            | StageCommand::Hide { interrupt, .. }
            | StageCommand::Background { interrupt, .. }
            | StageCommand::ClearLayer { interrupt, .. }
            | StageCommand::Movie { interrupt, .. } => interrupt,
            _ => panic!("unexpected stage command"),
        };
        assert_eq!(*interrupt, PresentationInterruptPolicy::ReplaceFromCurrent);
    }
}

#[test]
fn generated_tsuinosora_project_uses_current_native_platform_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let generated = Command::new("python")
        .args([
            "-c",
            "import sys; sys.path.insert(0, 'Tools/TsuiNoSora'); from tsuinosora_rendering import _render_nativevn_project; print(_render_nativevn_project([], []))",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let source = String::from_utf8(generated.stdout).unwrap();
    let targets = TargetManifest::from_project_yaml(&source).unwrap();
    let target = targets.find("tsuinosora-internal-game").unwrap();
    assert_eq!(target.platforms, ["windows", "linux", "macos", "android"]);
    let project: serde_yaml::Value = serde_yaml::from_str(&source).unwrap();
    let profile: PlatformHostProfile =
        serde_yaml::from_value(project["platform_profiles"]["windows-internal-release"].clone())
            .unwrap();
    validate_host_profile(&profile).unwrap();
    assert_eq!(profile.audio_mixer.providers, ["kira"]);
    assert_eq!(profile.audio_output.providers, ["wasapi"]);
    assert_eq!(profile.target, target.id);
}
