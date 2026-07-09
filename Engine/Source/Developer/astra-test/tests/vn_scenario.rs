use std::fs;

use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader};
use astra_test::{ScenarioRunner, ScenarioStatus};
use astra_vn::{compile_astra_sources, package_sections_for_story, AstraSource};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:prologue.hello speaker:hero voice:voice.hero.0001 #@id line.hello
    choice key:prologue.where #@id choice.where
      option key:choice.library -> library #@id choice.library
state library #@id state.library
  scene library #@id scene.library
    text key:library.followup speaker:hero voice:voice.hero.0002 #@id line.library
    jump ending.good #@id jump.good
"#;

const MOVIE_WAIT_STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait fallback:native-assets/movie/op_fallback.png #@id movie.opening
    text key:opening.after_movie speaker:narrator #@id line.after_movie
"#;

#[test]
fn vn_scenario_runs_full_player_route_from_package() {
    let root = tempfile::tempdir().unwrap();
    let scenario_dir = root.path().join("scenarios");
    fs::create_dir(&scenario_dir).unwrap();

    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let mut request = PackageBuildRequest::minimal("com.example.nativevn", "classic", sections);
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "classic",
            "runtime_provider": "native_vn",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
    .to_string()
    .into_bytes();
    request.scenario_refs = serde_json::json!({
        "schema": "astra.scenario_refs.v1",
        "scenarios": ["scenarios/vn_route.yaml"]
    })
    .to_string()
    .into_bytes();
    let package = PackageBuilder::build(request).unwrap();
    let reader = PackageReader::open(package.as_bytes()).unwrap();
    assert!(reader.has_section("vn.compiled_story"));
    let _: astra_vn::CompiledStory = reader
        .container()
        .decode_postcard("vn.compiled_story")
        .unwrap();
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();

    fs::write(
        scenario_dir.join("vn_route.yaml"),
        r#"
schema: astra.scenario.v1
stage: stage3-astra-vn
package: game.astrapkg
target: nativevn-game
profile: classic
seed: 7
locale: zh-Hans
actions:
  - launch: {}
  - advance: {}
  - choose: choice.library
  - advance: {}
  - replay_voice: voice.hero.0002
  - open_system: route_chart
  - save: slot.auto
  - load: slot.auto
  - replay_from_start: {}
assertions:
  - route_reached: ending.good
  - backlog_has_key: library.followup
  - read_state_has: line.library
  - voice_replay_available: voice.hero.0002
  - replay_hash_match: true
  - no_blocking_diagnostics: true
"#,
    )
    .unwrap();

    let report = ScenarioRunner::run_file(scenario_dir.join("vn_route.yaml")).unwrap();
    assert_eq!(
        report.status,
        ScenarioStatus::Pass,
        "{}\n{:?}",
        report.explain(),
        report.diagnostics
    );
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.route_coverage"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "player_route.full"));
    assert!(report.checks.iter().any(|check| {
        check.id == "runtime_provider.native_vn" && check.status == ScenarioStatus::Pass
    }));
}

#[test]
fn vn_scenario_completes_movie_await_through_real_player_input() {
    let root = tempfile::tempdir().unwrap();
    let scenario_dir = root.path().join("scenarios");
    fs::create_dir(&scenario_dir).unwrap();

    let compiled =
        compile_astra_sources([AstraSource::new("movie_wait.astra", MOVIE_WAIT_STORY)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let mut request = PackageBuildRequest::minimal("com.example.nativevn", "classic", sections);
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "classic",
            "runtime_provider": "native_vn",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
    .to_string()
    .into_bytes();
    request.scenario_refs = serde_json::json!({
        "schema": "astra.scenario_refs.v1",
        "scenarios": ["scenarios/movie_wait.yaml"]
    })
    .to_string()
    .into_bytes();
    let package = PackageBuilder::build(request).unwrap();
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();

    fs::write(
        scenario_dir.join("movie_wait.yaml"),
        r#"
schema: astra.scenario.v1
stage: stage3-astra-vn
package: game.astrapkg
target: nativevn-game
profile: classic
platform: headless
seed: 7
locale: zh-Hans
actions:
  - launch: {}
  - player_input:
      kind: advance
  - player_input:
      kind: complete_wait
      value: movie.opening.end
  - player_input:
      kind: save
      slot: slot.wait
  - player_input:
      kind: load
      slot: slot.wait
assertions:
  - backlog_has_key: opening.after_movie
  - no_blocking_diagnostics: true
"#,
    )
    .unwrap();

    let report = ScenarioRunner::run_file(scenario_dir.join("movie_wait.yaml")).unwrap();
    assert_eq!(
        report.status,
        ScenarioStatus::Pass,
        "{}\n{:?}",
        report.explain(),
        report.diagnostics
    );
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "player_route.full"));
}

#[test]
fn vn_scenario_supports_stage3_player_input_coverage_visual_and_hash_assertions() {
    let root = tempfile::tempdir().unwrap();
    let scenario_dir = root.path().join("scenarios");
    fs::create_dir(&scenario_dir).unwrap();

    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.push(astra_package::SectionPayload::raw(
        "tsuinosora.reference_evidence",
        "tsuinosora.reference_evidence.v1",
        serde_json::json!({
            "schema": "tsuinosora.reference_evidence.v1",
            "references": [{
                "id": "title",
                "hash": "sha256:title-reference",
                "width": 640,
                "height": 480,
                "regions": [{"id": "menu", "hash": "sha256:title-menu"}]
            }]
        })
        .to_string()
        .into_bytes(),
    ));
    let mut request = PackageBuildRequest::minimal("com.example.nativevn", "classic", sections);
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "classic",
            "runtime_provider": "native_vn",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
    .to_string()
    .into_bytes();
    request.scenario_refs = serde_json::json!({
        "schema": "astra.scenario_refs.v1",
        "scenarios": ["scenarios/stage3_route.yaml", "scenarios/stage3_route_hash.yaml"]
    })
    .to_string()
    .into_bytes();
    let package = PackageBuilder::build(request).unwrap();
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();

    let scenario_template = |hash_assertion: &str| {
        format!(
            r#"
schema: astra.scenario.v1
stage: stage3-astra-vn
package: game.astrapkg
target: nativevn-game
profile: classic
platform: windows
generated_route_id: route.library
mount_aliases:
  original: original_install_root
seed: 7
locale: zh-Hans
actions:
  - launch: {{}}
  - player_input:
      kind: advance
  - player_input:
      kind: choose
      value: choice.library
  - player_input:
      kind: advance
  - player_input:
      kind: replay_voice
      value: voice.hero.0002
  - player_input:
      kind: open_system
      value: route_chart
  - player_input:
      kind: save
      slot: slot.auto
  - player_input:
      kind: load
      slot: slot.auto
  - player_input:
      kind: set_auto
      value: "true"
  - player_input:
      kind: set_skip
      value: read
  - player_input:
      kind: set_config
      key: text_speed
      value: instant
  - player_input:
      kind: unlock_gallery
      value: cg.opening
  - replay_from_start: {{}}
assertions:
  - coverage:
      routes: [ending.good]
      backlog_keys: [library.followup]
      read_state: [line.library]
      voice_replay: [voice.hero.0002]
  - system_state:
      auto_enabled: true
      skip_mode: read
      config:
        text_speed: instant
      gallery_unlocks: [cg.opening]
  - visual_reference:
      id: title
      hash: sha256:title-reference
      regions: [menu]
{hash_assertion}
  - no_blocking_diagnostics: true
"#
        )
    };
    fs::write(
        scenario_dir.join("stage3_route.yaml"),
        scenario_template(""),
    )
    .unwrap();

    let report = ScenarioRunner::run_file(scenario_dir.join("stage3_route.yaml")).unwrap();
    assert_eq!(report.status, ScenarioStatus::Pass, "{}", report.explain());
    assert_eq!(report.platform.as_deref(), Some("windows"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "assert.visual_reference.title"));

    let hash_assertion = format!(
        "  - hash:\n      state: {}\n      event: {}\n      presentation: {}\n",
        report.hashes.state, report.hashes.event, report.hashes.presentation
    );
    fs::write(
        scenario_dir.join("stage3_route_hash.yaml"),
        scenario_template(&hash_assertion),
    )
    .unwrap();
    let hash_report =
        ScenarioRunner::run_file(scenario_dir.join("stage3_route_hash.yaml")).unwrap();
    assert_eq!(
        hash_report.status,
        ScenarioStatus::Pass,
        "{}",
        hash_report.explain()
    );
    assert!(hash_report
        .checks
        .iter()
        .any(|check| check.id == "assert.hash"));
}
