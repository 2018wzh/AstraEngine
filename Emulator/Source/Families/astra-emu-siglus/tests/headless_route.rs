//! Rewrite+ representative path: animated title -> Start -> copyright/intro ->
//! title -> Start -> opening movie -> prologue. Uses real native wall time.
mod common;
use common::*;
use std::time::Duration;

#[test]
#[ignore = "requires a writable authorized Rewrite+ copy, hardware GPU and audio device"]
fn native_rewrite_representative_route() {
    let session = Native::open();
    rewrite_opening(&session);
    let mut snapshots = std::collections::BTreeSet::new();
    let mut visible = false;
    for _ in 0..120 {
        session.click(640.0, 400.0);
        session.wait(Duration::from_millis(640));
        let frame = session.frame();
        let (sum, lit) = digest(&frame);
        snapshots.insert(sum);
        visible |= lit > frame.pixels().len() / 400;
    }
    assert!(
        visible && snapshots.len() > 1,
        "representative route produces changing complete native frames"
    );
    session.close();
}
