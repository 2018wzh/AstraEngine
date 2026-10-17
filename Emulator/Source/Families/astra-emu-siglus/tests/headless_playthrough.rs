//! Rewrite+ route attempt and native quick-save/load checks. A route timeout is
//! not completion. Native savedata is written only in the explicit test copy.
mod common;
use astra_emu_abi as abi;
use common::*;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn slots() -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let directory = game().join("savedata");
    if !directory.exists() {
        return BTreeMap::new();
    }
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "sav")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|p| {
            let data = std::fs::read(&p).unwrap();
            (p, data)
        })
        .collect()
}
fn saved_session() -> Native {
    let session = Native::open();
    rewrite_opening(&session);
    let before = slots();
    // These are the observed 1280x720 game controls, including its rendered
    // confirmation dialog. No Host window/modal result is synthesized.
    session.click(1140.0, 587.0);
    session.wait(Duration::from_secs(2));
    session.click(1140.0, 587.0);
    session.wait(Duration::from_secs(2));
    session.snapshot("save-confirmation");
    session.click(580.0, 335.0);
    session.wait(Duration::from_secs(3));
    let saved = slots();
    assert!(
        saved
            .iter()
            .any(|(path, bytes)| before.get(path) != Some(bytes)),
        "native quick-save must change a real numeric slot"
    );
    eprintln!("native numeric save slots: {}", saved.len());
    let context = session.state().context_epoch;
    session.click(1220.0, 587.0);
    session.wait(Duration::from_secs(2));
    session.click(1220.0, 587.0);
    session.wait(Duration::from_secs(2));
    session.snapshot("load-confirmation");
    session.click(580.0, 335.0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.state().context_epoch == context {
        assert!(
            Instant::now() < deadline,
            "native quick-load must publish a successful context reset"
        );
        session.wait(Duration::from_millis(20));
    }
    session.snapshot("loaded");
    eprintln!(
        "native load context: {context} -> {}",
        session.state().context_epoch
    );
    session
}

#[test]
#[ignore = "requires a writable authorized Rewrite+ copy, hardware GPU and audio device"]
fn native_rewrite_save_load_close_reopen() {
    let session = saved_session();
    let first = session.frame();
    let original = first.pixels().to_vec();
    let mut digests = std::collections::BTreeSet::new();
    for _ in 0..60 {
        session.click(640.0, 400.0);
        session.wait(Duration::from_millis(640));
        digests.insert(digest(&session.frame()).0);
    }
    assert!(digests.len() > 1, "native story continues after load");
    session.snapshot("continued");
    session.close();
    assert_eq!(first.pixels(), original, "pre-close lease stays immutable");
    let reopened = Native::open();
    reopened.frame();
    reopened.close();
    assert_eq!(first.pixels(), original, "lease survives same-DLL reopen");
}

#[test]
#[ignore = "requires a writable authorized Rewrite+ copy, hardware GPU and audio device"]
fn native_rewrite_save_load_and_route_attempt() {
    let session = saved_session();
    let budget = std::env::var("ASTRA_SIGLUS_TEST_PLAYTHROUGH_BUDGET")
        .map(|s| s.parse::<u64>().expect("integer observation budget"))
        .unwrap_or(400_000);
    assert!(budget > 0);
    let mut finished = false;
    for index in 0..budget {
        if session.state().state == abi::SESSION_FINISHED {
            finished = true;
            break;
        }
        if index % 12 == 0 {
            session.click(640.0, 400.0);
            session.key("Enter");
        }
        session.wait(Duration::from_millis(16));
    }
    session.close();
    assert!(
        finished,
        "native route did not finish within the observation budget"
    );
    let reopened = Native::open();
    reopened.frame();
    reopened.close();
}
