use std::fs;

use astra_core::Hash256;
use astra_package::{
    ContainerBlob, PackageBuildRequest, PackageBuilder, PackageReader, ScenarioReference,
    ScenarioRefsManifest, SectionPayload,
};
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

story system.route_chart #@id story.system.route_chart
state route_chart #@id state.system.route_chart
  scene route_chart #@id scene.system.route_chart
    system_page kind:route_chart policy:astra.policy.standard #@id page.route_chart
"#;

const MOVIE_WAIT_STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:asset:/movie/op end:wait fence:movie.opening.end fallback:asset:/movie/op_fallback #@id movie.opening
    text key:opening.after_movie speaker:narrator #@id line.after_movie
"#;

fn build_test_package(
    sections: Vec<SectionPayload>,
    profile: &str,
    scenarios: &[(&str, &[u8])],
) -> ContainerBlob {
    let mut request = PackageBuildRequest::fixture("com.example.nativevn", profile, sections);
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": profile,
            "runtime_provider": "native_vn",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
    .to_string()
    .into_bytes();
    let mut policy: astra_plugin_abi::ProviderPolicy =
        serde_json::from_slice(&request.provider_policy).unwrap();
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    let provider_bindings = registry
        .bindings
        .iter()
        .map(|binding| {
            let mut context = binding.context.clone();
            context.target = "nativevn-game".to_string();
            context.profile = profile.to_string();
            astra_plugin_abi::ProviderBinding::new(
                binding.slot.clone(),
                binding.provider_id.clone(),
                context,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    policy.profile = profile.to_string();
    policy.bindings = provider_bindings.clone();
    registry.bindings = provider_bindings;
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
    let mut bindings = Vec::new();
    for (path, payload) in scenarios {
        let section_id = format!(
            "scenario.ref.{}",
            Hash256::from_sha256(path.as_bytes()).to_hex()
        );
        request.cooked_assets.push(SectionPayload::raw(
            section_id.clone(),
            "astra.scenario.v1",
            payload.to_vec(),
        ));
        bindings.push(ScenarioReference {
            path: (*path).to_string(),
            section_id,
            hash: Hash256::from_sha256(payload),
            byte_size: payload.len() as u64,
        });
    }
    request.scenario_refs = serde_json::to_vec(&ScenarioRefsManifest {
        schema: "astra.scenario_refs.v2".to_string(),
        scenarios: bindings,
    })
    .unwrap();
    PackageBuilder::build(request).unwrap()
}

#[test]
fn vn_scenario_runs_full_player_route_from_package() {
    let root = tempfile::tempdir().unwrap();
    let scenario_dir = root.path().join("scenarios");
    fs::create_dir(&scenario_dir).unwrap();

    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let scenario = r#"
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
"#;
    let package = build_test_package(
        sections,
        "classic",
        &[("scenarios/vn_route.yaml", scenario.as_bytes())],
    );
    let reader = PackageReader::open(package.as_bytes()).unwrap();
    assert!(reader.has_section("vn.compiled_story"));
    let _: astra_vn::CompiledStory = reader
        .container()
        .decode_postcard("vn.compiled_story")
        .unwrap();
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();

    fs::write(scenario_dir.join("vn_route.yaml"), scenario).unwrap();

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
    let scenario = r#"
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
"#;
    let package = build_test_package(
        sections,
        "classic",
        &[("scenarios/movie_wait.yaml", scenario.as_bytes())],
    );
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();
    fs::write(scenario_dir.join("movie_wait.yaml"), scenario).unwrap();

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
    let first_scenario = scenario_template("");
    fs::write(scenario_dir.join("stage3_route.yaml"), &first_scenario).unwrap();
    let package = build_test_package(
        sections.clone(),
        "classic",
        &[("scenarios/stage3_route.yaml", first_scenario.as_bytes())],
    );
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();

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
    let hash_scenario = scenario_template(&hash_assertion);
    fs::write(scenario_dir.join("stage3_route_hash.yaml"), &hash_scenario).unwrap();
    let package = build_test_package(
        sections,
        "classic",
        &[
            ("scenarios/stage3_route.yaml", first_scenario.as_bytes()),
            ("scenarios/stage3_route_hash.yaml", hash_scenario.as_bytes()),
        ],
    );
    fs::write(root.path().join("game.astrapkg"), package.as_bytes()).unwrap();
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
