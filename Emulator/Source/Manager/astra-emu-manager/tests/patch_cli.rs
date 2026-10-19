#[path = "../../../Tools/astra-emu-patch/tests/fixtures/mod.rs"]
mod fixtures;

use fixtures::PublicOverlay;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_astra-emu-manager"));
    command.arg("patch");
    command
}

fn apply(fixture: &PublicOverlay) -> Command {
    let mut command = binary();
    command
        .arg("apply")
        .arg("--script")
        .arg(&fixture.script)
        .arg("--source")
        .arg(&fixture.source)
        .arg("--patch")
        .arg(&fixture.patch)
        .arg("--output")
        .arg(&fixture.output);
    command
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fingerprints(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut hashes = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                hashes.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    digest(&fs::read(path).unwrap()),
                );
            }
        }
    }
    hashes
}

fn failure(output: Output, code: i32, diagnostic: &str) {
    assert_eq!(output.status.code(), Some(code));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(diagnostic), "{stderr}");
}

fn assert_no_staging(root: &Path) {
    for entry in fs::read_dir(root).unwrap() {
        assert!(!entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".astra-patch-staging-"));
    }
}

#[test]
fn golden_pe_overlay_converts_complete_bytes_and_keeps_all_inputs_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("公开 fixture");
    fs::create_dir(&root).unwrap();
    let fixture = PublicOverlay::new(&root);
    let source_before = fingerprints(&fixture.source);
    let patch_before = fingerprints(&fixture.patch);
    let script_before = fs::read(&fixture.script).unwrap();
    let output = apply(&fixture).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut reported: Vec<_> = stdout
        .lines()
        .map(|line| {
            let (_, path) = line.split_once(": ").expect("result location");
            fs::canonicalize(path).unwrap()
        })
        .collect();
    let mut expected = vec![
        fs::canonicalize(&fixture.output).unwrap(),
        fs::canonicalize(fixture.output.join("astra-patch-result.json")).unwrap(),
    ];
    reported.sort();
    expected.sort();
    assert_eq!(reported, expected);
    assert_eq!(
        fs::read(fixture.output.join("assets.bin")).unwrap(),
        fixture.expected
    );
    assert_eq!(
        fs::read(fixture.output.join("unchanged.dat")).unwrap(),
        b"complete independent public fixture"
    );
    assert_eq!(
        fs::read(fixture.output.join("zero.dat")).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(fingerprints(&fixture.output).len(), 4);
    fixture.assert_inputs();
    assert_eq!(fingerprints(&fixture.source), source_before);
    assert_eq!(fingerprints(&fixture.patch), patch_before);
    assert_eq!(fs::read(&fixture.script).unwrap(), script_before);

    let receipt_bytes = fs::read(fixture.output.join("astra-patch-result.json")).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["schema"], "astra.emu.file_patch.v1");
    assert_eq!(
        receipt["recipe"],
        json!({"id":"public-pe-overlay", "version":1})
    );
    assert_eq!(
        receipt["profile"],
        json!({
            "plugin_id":"public.fixture", "configuration":{"script_encoding":"utf8"},
        })
    );
    let inputs = receipt["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 4);
    for input in inputs {
        let (hashes, input_root) = match input["root"].as_str().unwrap() {
            "source" => (&source_before, &fixture.source),
            "patch" => (&patch_before, &fixture.patch),
            other => panic!("unexpected public input root {other}"),
        };
        let path = Path::new(input["path"].as_str().unwrap());
        assert!(!path.is_absolute());
        assert_eq!(input["sha256"].as_str().unwrap(), hashes[path]);
        assert_eq!(
            input["sha256"],
            digest(&fs::read(input_root.join(path)).unwrap())
        );
    }
    assert_eq!(
        receipt["outputs"],
        json!([
            {"path":"assets.bin", "size":fixture.expected.len(), "sha256":digest(&fixture.expected)},
            {"path":"unchanged.dat", "size":35, "sha256":digest(b"complete independent public fixture")},
            {"path":"zero.dat", "size":0, "sha256":digest(&[])},
        ])
    );
    assert!(!String::from_utf8(receipt_bytes)
        .unwrap()
        .contains(&root.to_string_lossy().to_string()));
    assert_no_staging(&root);
}

#[test]
fn changed_source_fingerprint_rejects_without_publishing_or_modifying_inputs() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    let mut changed = fs::read(fixture.source.join("assets.bin")).unwrap();
    changed[17] ^= 1;
    fs::write(fixture.source.join("assets.bin"), changed).unwrap();
    let source_before = fingerprints(&fixture.source);
    let patch_before = fingerprints(&fixture.patch);
    failure(
        apply(&fixture).output().unwrap(),
        1,
        "ASTRA_EMU_PATCH_UNSUPPORTED_INPUT",
    );
    assert!(!fixture.output.exists());
    assert_eq!(fingerprints(&fixture.source), source_before);
    assert_eq!(fingerprints(&fixture.patch), patch_before);
    assert_no_staging(root.path());
}

#[test]
fn existing_output_and_its_user_files_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    fs::create_dir(&fixture.output).unwrap();
    fs::write(
        fixture.output.join("user-owned.dat"),
        b"preserve this user file",
    )
    .unwrap();
    let before = fingerprints(&fixture.output);
    failure(
        apply(&fixture).output().unwrap(),
        1,
        "ASTRA_EMU_PATCH_OUTPUT_EXISTS",
    );
    assert_eq!(fingerprints(&fixture.output), before);
    fixture.assert_inputs();
    assert_no_staging(root.path());
}

#[test]
fn omitted_patch_uses_source_directory() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    fs::copy(
        fixture.patch.join("overlay.exe"),
        fixture.source.join("overlay.exe"),
    )
    .unwrap();
    let before = fingerprints(&fixture.source);
    let output = binary()
        .arg("apply")
        .arg("--script")
        .arg(&fixture.script)
        .arg("--source")
        .arg(&fixture.source)
        .arg("--output")
        .arg(&fixture.output)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.output.join("assets.bin")).unwrap(),
        fixture.expected
    );
    assert_eq!(fingerprints(&fixture.source), before);
}

fn write_catalog(root: &Path) -> Vec<Value> {
    let records = vec![
        json!({"id":"invalid-syntax", "title":"Invalid syntax", "script":"invalid.luau"}),
        json!({"id":"would-error", "title":"Erroring script", "script":"error.luau"}),
        json!({"id":"not-installed", "title":"Missing script", "script":"missing.luau"}),
    ];
    fs::write(
        root.join("catalog.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    fs::write(root.join("invalid.luau"), b"not valid Luau @@@").unwrap();
    fs::write(
        root.join("error.luau"),
        b"error('catalog listing executed a script')",
    )
    .unwrap();
    records
}

fn assert_listing(output: Output, records: &[Value]) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(actual, records);
}

#[test]
fn catalog_list_does_not_open_compile_execute_or_probe_scripts() {
    let root = tempfile::tempdir().unwrap();
    let records = write_catalog(root.path());
    let before = fingerprints(root.path());
    assert_listing(
        binary()
            .arg("list")
            .arg("--scripts")
            .arg(root.path())
            .output()
            .unwrap(),
        &records,
    );
    assert_eq!(fingerprints(root.path()), before);
}

#[test]
fn default_catalog_is_executable_adjacent_not_current_directory() {
    let root = tempfile::tempdir().unwrap();
    let scripts = root.path().join("patches");
    let unrelated = root.path().join("unrelated-current-directory");
    fs::create_dir(&scripts).unwrap();
    fs::create_dir(&unrelated).unwrap();
    let records = write_catalog(&scripts);
    let executable = root
        .path()
        .join(format!("astra-emu-manager{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_astra-emu-manager"), &executable).unwrap();
    assert_listing(
        Command::new(executable)
            .current_dir(unrelated)
            .arg("patch")
            .arg("list")
            .output()
            .unwrap(),
        &records,
    );
}

#[test]
fn invalid_catalog_relative_paths_are_rejected_before_listing() {
    let root = tempfile::tempdir().unwrap();
    for script in [
        "../escape.luau",
        "/absolute.luau",
        "C:\\absolute.luau",
        "name:stream.luau",
    ] {
        fs::write(
            root.path().join("catalog.json"),
            serde_json::to_vec(&json!([
                {"id":"unsafe", "title":"Unsafe path", "script":script},
            ]))
            .unwrap(),
        )
        .unwrap();
        failure(
            binary()
                .arg("list")
                .arg("--scripts")
                .arg(root.path())
                .output()
                .unwrap(),
            1,
            "ASTRA_EMU_PATCH_PATH",
        );
    }
}

#[test]
fn recipe_failure_keeps_relative_script_line_and_cause_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    fs::write(&fixture.script, b"return {id='failure',version=1,title='Public failure',apply=function(ctx)\nctx.fail('INTEGRITY','public integrity marker')\nend}").unwrap();
    let output = apply(&fixture).output().unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    failure(output, 1, "ASTRA_EMU_PATCH_INTEGRITY");
    assert!(stderr.contains("public integrity marker"));
    assert!(stderr.contains("public-overlay.luau:2"), "{stderr}");
    assert!(!stderr.contains(&root.path().to_string_lossy().to_string()));
    assert!(!fixture.output.exists());
}

#[test]
fn cancellation_error_returns_130_and_rolls_back_staged_files() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    fs::write(&fixture.script, b"return {id='cancel',version=1,title='Public cancellation',apply=function(ctx)\nctx.copy('source','assets.bin','assets.bin')\nctx.fail('CANCELLED','public cancellation')\nend}").unwrap();
    failure(
        apply(&fixture).output().unwrap(),
        130,
        "ASTRA_EMU_PATCH_CANCELLED",
    );
    assert!(!fixture.output.exists());
    fixture.assert_inputs();
    assert_no_staging(root.path());
}

#[test]
fn dense_progress_does_not_drop_completion_or_block_worker_join() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    fs::write(&fixture.script, b"return {id='progress',version=1,title='Public progress',apply=function(ctx)\nfor i=1,50000 do ctx.progress('public progress',i,50000) end\nctx.copy('source','assets.bin','assets.bin')\nreturn {plugin_id='public.fixture',configuration={}}\nend}").unwrap();
    let output = apply(&fixture).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.output.join("assets.bin")).unwrap(),
        fixture.original
    );
    assert!(fixture.output.join("astra-patch-result.json").is_file());
    fixture.assert_inputs();
}

#[test]
fn command_line_errors_return_one_and_help_returns_zero() {
    let error = binary().arg("apply").output().unwrap();
    assert_eq!(error.status.code(), Some(1));
    assert!(error.stdout.is_empty());
    let output = binary().arg("--help").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}

#[test]
fn corrupt_overlay_boundaries_roll_back_without_changing_inputs() {
    for case in ["crc", "truncated-zlib", "declared-length"] {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = PublicOverlay::new(root.path());
        let expected = match case {
            "crc" => {
                fixture.blocks[1].2 ^= 1;
                "ASTRA_EMU_PATCH_INTEGRITY"
            }
            "truncated-zlib" => {
                let length = fixture.blocks[2].3.len() - 2;
                fixture.blocks[2].3.truncate(length);
                "ASTRA_EMU_PATCH_CODEC"
            }
            "declared-length" => {
                fixture.blocks[0].1 -= 1;
                "ASTRA_EMU_PATCH_CODEC"
            }
            _ => unreachable!(),
        };
        fixture.write_patch();
        // Keep fingerprints valid so each failure reaches the intended native
        // decoder/integrity boundary rather than failing recipe identification.
        fixture.write_script();
        let source_before = fingerprints(&fixture.source);
        let patch_before = fingerprints(&fixture.patch);
        failure(apply(&fixture).output().unwrap(), 1, expected);
        assert!(!fixture.output.exists());
        assert_eq!(fingerprints(&fixture.source), source_before);
        assert_eq!(fingerprints(&fixture.patch), patch_before);
        assert_no_staging(root.path());
    }
}
