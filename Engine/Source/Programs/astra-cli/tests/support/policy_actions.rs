use super::*;

#[test]
fn packaged_policy_reads_authorized_text_without_input_and_restores_pending_save() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "nativevn-policy-action");
    let project = write_nativevn_minimal_fixture(root, &case_dir);
    fs::create_dir_all(case_dir.join("Policies")).unwrap();
    fs::write(case_dir.join("Policies/note.luau"), r#"
astra.command.register("load_note", {schema="demo.note.v1",fields={resource={kind="asset_uri",required=true}}}, function(args)
    astra.mutate.set_var("project","loaded", (astra.var.get("project","loaded") or 0) + 1)
    local text,status = astra.host.read_text("note", args.resource)
    if status == "pending" then return astra.action.await("note") end
    if status == "ready" then assert(text == "hello policy") else assert(status == "interrupted") end
    assert(astra.var.get("project","loaded") == 1)
    return astra.action.complete()
end)
"#).unwrap();
    let story = nativevn_minimal_story().replacen("    text key:line.hello", "    mutate project.loaded = 0 #@id loaded.init\n    load_note resource:asset:/text/note #@id note.load\n    text key:line.hello",1);
    fs::write(case_dir.join("Scripts/main.astra"), story).unwrap();
    let mut configuration: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&project).unwrap()).unwrap();
    configuration["nativevn"]["policy_actions"] =
        serde_yaml::from_str("[{entry: Policies/note.luau, read_text: ['asset:/text/note']}]")
            .unwrap();
    fs::write(&project, serde_yaml::to_string(&configuration).unwrap()).unwrap();
    fs::write(case_dir.join("Assets/note.txt"), "hello policy").unwrap();
    fs::write(case_dir.join("Assets/note.txt.astra-asset.yaml"), format!("schema: astra.asset.v1\nid: asset:/text/note\nsource: Assets/note.txt\nsource_hash: {}\ntype: text\nlicense: CC0-1.0\nimporter: astra.import.text\ncook:\n  processor: astra.cook.binary\n  target_profiles: [minimal]\n  params: {{}}\nreview: accepted\n",Hash256::from_sha256(b"hello policy"))).unwrap();
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("policy.astrapkg");
    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "minimal",
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            cooked.to_str().unwrap(),
        ],
    );
    run_astra(
        root,
        [
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            package.to_str().unwrap(),
        ],
    );
    let bytes = fs::read(&package).unwrap();
    let reader = PackageReader::open(&bytes).unwrap();
    assert!(reader.has_section("vn.policy_actions"));
    let mut player = astra_player_vn::NativeVnHostCommandSource::from_package(
        &reader,
        astra_vn_core::VnRunConfig {
            profile: "minimal".into(),
            locale: "en".into(),
        },
        320,
        180,
        astra_player_core::PlayerHostResourceId(1),
    )
    .unwrap();
    player.launch().unwrap();
    assert_eq!(player.pending_wait().unwrap().command_id, "note.load");
    player.tick_policy().unwrap();
    player
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    player
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
        .unwrap();
    let pending = player.save("slot.01").unwrap();
    let mut broken = pending.clone();
    broken.truncate(broken.len() / 2);
    assert!(player.restore(&broken).is_err());
    assert_eq!(player.pending_wait().unwrap().command_id, "note.load");
    player.restore(&pending).unwrap();
    player.tick_policy().unwrap();
    assert_eq!(player.pending_wait().unwrap().command_id, "line.hello");
    player
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    player
        .prepare_save_metadata("slot.02", "2000-01-01T00:00:01Z".into(), 1)
        .unwrap();
    let resumed = player.save("slot.02").unwrap();
    player.restore(&resumed).unwrap();
    player
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    player
        .prepare_save_metadata("slot.03", "2000-01-01T00:00:02Z".into(), 2)
        .unwrap();
    player.save("slot.03").unwrap();
    player.release_resources().unwrap();
    player.shutdown().unwrap();
    let report = run_nativevn_headless(&package, root, &case_dir, "minimal", "Digit1");
    assert_eq!(report["status"], "passed");

    // An action cannot obtain a different resource merely by changing its typed argument.
    let story_path = case_dir.join("Scripts/main.astra");
    let denied_story = fs::read_to_string(&story_path)
        .unwrap()
        .replace("resource:asset:/text/note", "resource:asset:/text/denied");
    fs::write(&story_path, denied_story).unwrap();
    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "minimal",
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            cooked.to_str().unwrap(),
        ],
    );
    run_astra(
        root,
        [
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            package.to_str().unwrap(),
        ],
    );
    let bytes = fs::read(&package).unwrap();
    let reader = PackageReader::open(&bytes).unwrap();
    let mut denied = astra_player_vn::NativeVnHostCommandSource::from_package(
        &reader,
        astra_vn_core::VnRunConfig {
            profile: "minimal".into(),
            locale: "en".into(),
        },
        320,
        180,
        astra_player_core::PlayerHostResourceId(1),
    )
    .unwrap();
    denied.launch().unwrap();
    assert!(denied
        .tick_policy()
        .unwrap_err()
        .to_string()
        .contains("ASTRA_POLICY_HOST_IO_DENIED"));
    denied.release_resources().unwrap();
    denied.shutdown().unwrap();

    // Missing author IDs fail Cook and do not replace the preceding cooked output.
    let old_manifest = fs::read(cooked.join("cook_manifest.yaml")).unwrap();
    configuration["nativevn"]["policy_actions"] =
        serde_yaml::from_str("[{entry: Policies/note.luau, read_text: ['asset:/text/missing']}]")
            .unwrap();
    fs::write(&project, serde_yaml::to_string(&configuration).unwrap()).unwrap();
    let blocked = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "minimal",
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            cooked.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("ASTRA_VN_POLICY_TEXT_ASSET_MAPPING"));
    assert_eq!(
        fs::read(cooked.join("cook_manifest.yaml")).unwrap(),
        old_manifest
    );
    fs::remove_dir_all(case_dir).unwrap();
}
