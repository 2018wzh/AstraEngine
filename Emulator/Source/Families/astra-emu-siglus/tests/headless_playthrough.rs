//! Rewrite+ controlled route observation and native quick-save/load checks.
//! Process exit is not ending evidence. Savedata stays in the explicit test copy.
mod common;
use common::*;
use std::io::{BufRead, Write};
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
fn native_rewrite_controlled_route() {
    let mut session = Some(Native::open());
    eprintln!("route ready: wait MS | click X Y | key NAME MS | snapshot NAME | reopen | close");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let words: Vec<_> = line.split_whitespace().collect();
        eprintln!("route command: {line}");
        match words.as_slice() {
            ["wait", milliseconds] => {
                session.as_ref().unwrap().wait(Duration::from_millis(
                    milliseconds.parse().expect("integer milliseconds"),
                ));
            }
            ["click", x, y] => session.as_ref().unwrap().click(
                x.parse().expect("x coordinate"),
                y.parse().expect("y coordinate"),
            ),
            ["key", name, milliseconds] => {
                let native = session.as_ref().unwrap();
                let mut press = native.event(astra_emu_abi::INPUT_KEY);
                press.physical_key = astra_emu_abi::Bytes::new(name.as_bytes());
                press.logical_key = press.physical_key;
                press.pressed = 1;
                native.send(&[press]);
                native.wait(Duration::from_millis(
                    milliseconds.parse().expect("integer milliseconds"),
                ));
                press.pressed = 0;
                native.send(&[press]);
            }
            ["snapshot", name] => {
                assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
                let native = session.as_ref().unwrap();
                native.snapshot(name);
                eprintln!("route state: {:?}", native.state());
            }
            ["reopen"] => {
                session.take().unwrap().close();
                session = Some(Native::open());
            }
            ["close"] => {
                session.take().unwrap().close();
                eprintln!("route closed; ending requires separate visual evidence");
                break;
            }
            _ => panic!("invalid route command: {line}"),
        }
        eprintln!("route command complete: {line}");
        std::io::stderr().flush().unwrap();
    }
    if let Some(native) = session {
        native.close();
    }
}
