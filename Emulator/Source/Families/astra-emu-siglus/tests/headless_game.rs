//! Real native C ABI smoke: hardware GPU and native audio device required.
//! Audio is played by Kira/CPAL; this test does not substitute a counting sink.
mod common;
use astra_emu_abi as abi;
use common::*;
use std::time::Duration;

#[test]
#[ignore = "requires an authorized writable Siglus game copy, hardware GPU and audio device"]
fn native_boot_input_pause_frames_close_reopen() {
    let session = Native::open();
    assert_eq!(session.info.logical, session.info.raster);
    let first = session.frame();
    let original = first.pixels().to_vec();
    let mut digests = std::collections::BTreeSet::new();
    let mut lit = 0;
    for index in 0..150 {
        if index % 30 == 0 { session.click(640.0, 400.0); }
        if index % 30 == 15 { session.key("Enter"); }
        session.wait(Duration::from_millis(40));
        let (sum, count) = digest(&session.frame()); digests.insert(sum); lit = lit.max(count);
    }
    assert!(lit > first.pixels().len() / 400, "native complete frame has visible content");
    assert!(digests.len() > 1, "native output changes without host advance");
    assert_eq!(first.pixels(), original, "old CPU lease is immutable");
    session.control(abi::CONTROL_PAUSE); session.wait_state(abi::SESSION_PAUSED);
    session.wait(Duration::from_millis(100));
    let paused = session.frame();
    session.wait(Duration::from_millis(200));
    assert_eq!(session.frame().identity(), paused.identity());
    session.control(abi::CONTROL_RESUME); session.wait_state(abi::SESSION_RUNNING);
    let mut window = session.event(abi::INPUT_WINDOW);
    window.window.drawable_width = 1920; window.window.drawable_height = 1080;
    window.window.focused = 0; session.send(&[window]); session.wait_state(abi::SESSION_PAUSED);
    assert_eq!(session.frame().info().raster, session.info.raster, "O resize does not change R");
    window.window.focused = 1; window.window.suspended = 1; session.send(&[window]); session.wait_state(abi::SESSION_PAUSED);
    window.window.suspended = 0; session.send(&[window]); session.wait_state(abi::SESSION_RUNNING);
    session.close();
    assert_eq!(first.pixels(), original, "CPU lease survives native session close");
    std::thread::spawn(move || drop(first)).join().unwrap();
    drop(paused);
    let reopened = Native::open(); reopened.frame(); reopened.close();
}
