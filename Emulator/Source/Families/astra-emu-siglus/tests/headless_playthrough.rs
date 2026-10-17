//! Rewrite+ route attempt and native quick-save/load checks. A route timeout is
//! not completion. Native savedata is written only in the explicit test copy.
mod common;
use astra_emu_abi as abi;
use common::*;
use std::{collections::BTreeMap, time::{Duration, Instant}};

fn slots() -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let directory = game().join("savedata");
    if !directory.exists() { return BTreeMap::new(); }
    std::fs::read_dir(directory).unwrap().map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sav") && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())))
        .map(|p| { let data = std::fs::read(&p).unwrap(); (p, data) }).collect()
}
#[test]
#[ignore = "requires a writable authorized Rewrite+ copy, hardware GPU and audio device"]
fn native_rewrite_save_load_and_route_attempt() {
    let session = Native::open(); rewrite_opening(&session);
    let before = slots();
    session.click(1120.0, 558.0); session.wait(Duration::from_secs(2));
    session.key("Escape"); session.wait(Duration::from_secs(1));
    let saved = slots();
    assert!(saved.iter().any(|(path, bytes)| before.get(path) != Some(bytes)), "native quick-save must change a real numeric slot");
    let context = session.state().context_epoch;
    session.click(1210.0, 558.0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.state().context_epoch == context {
        assert!(Instant::now() < deadline, "native quick-load must publish a successful context reset");
        session.wait(Duration::from_millis(20));
    }
    session.frame();
    let budget = std::env::var("ASTRA_SIGLUS_TEST_PLAYTHROUGH_BUDGET").map(|s| s.parse::<u64>().expect("integer observation budget")).unwrap_or(400_000);
    assert!(budget > 0);
    let mut finished = false;
    for index in 0..budget {
        if session.state().state == abi::SESSION_FINISHED { finished = true; break; }
        if index % 12 == 0 { session.click(640.0, 400.0); session.key("Enter"); }
        session.wait(Duration::from_millis(16));
    }
    session.close();
    assert!(finished, "native route did not finish within the observation budget");
    let reopened = Native::open(); reopened.frame(); reopened.close();
}
