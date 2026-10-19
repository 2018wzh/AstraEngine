//! Hand-coded public aPLib vectors through the real conversion API.
use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};

fn run(
    bytes: &[u8],
    expected: usize,
) -> (
    tempfile::TempDir,
    Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError>,
) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("public.aplib"), bytes).unwrap();
    let script = root.path().join("aplib.luau");
    fs::write(
        &script,
        format!(
            r#"
return {{id="public-aplib",version=1,title="Public aPLib",apply=function(ctx)
 local source=ctx.open("source","public.aplib")
 local bytes=ctx.codec.aplib(source:read(0,source:size()),{expected})
 local output=ctx.create("plain.bin")
 output:write(0,bytes)
 output:close()
 return {{plugin_id="public.aplib",configuration={{}}}}
end}}
"#
        ),
    )
    .unwrap();
    let result = apply(
        PatchRequest {
            script,
            source: source.clone(),
            patch: source,
            output: root.path().join("output"),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );
    (root, result)
}

#[test]
fn short_backreference_repeats_a_prior_literal_then_requires_eof() {
    // Tag 11011000: short match(offset=1,length=2), EOF(offset=0).
    let (_root, result) = run(&[b'A', 0xd8, 2, 0], 3);
    assert_eq!(
        fs::read(result.unwrap().output.join("plain.bin")).unwrap(),
        b"AAA"
    );
}

#[test]
fn backreference_before_the_first_literal_is_rejected() {
    let (root, result) = run(&[b'A', 0xc0, 4], 3);
    assert_eq!(result.err().unwrap().kind(), PatchErrorKind::Codec);
    assert!(!root.path().join("output").exists());
}

#[test]
fn eof_does_not_hide_trailing_packed_bytes() {
    let (root, result) = run(&[b'A', 0xc0, 0, 0], 1);
    assert_eq!(result.err().unwrap().kind(), PatchErrorKind::Codec);
    assert!(!root.path().join("output").exists());
}

#[test]
fn overlapping_copy_cannot_exceed_the_declared_output() {
    let (root, result) = run(&[b'A', 0xd8, 2, 0], 2);
    assert_eq!(result.err().unwrap().kind(), PatchErrorKind::Codec);
    assert!(!root.path().join("output").exists());
}
