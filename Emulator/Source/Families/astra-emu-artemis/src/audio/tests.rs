use super::*;
use std::time::{Duration, Instant};
struct Bytes(Vec<u8>);
impl MediaSource for Bytes {
    fn len(&self) -> Result<u64, String> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&self, offset: u64, out: &mut [u8]) -> Result<usize, String> {
        let source = self.0.get(offset as usize..).ok_or("offset")?;
        let n = out.len().min(source.len());
        out[..n].copy_from_slice(&source[..n]);
        Ok(n)
    }
}
fn wave() -> Vec<u8> {
    let mut b = b"RIFF".to_vec();
    b.extend((36u32 + 16000).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(16000u32.to_le_bytes());
    for frame in 0..8000 {
        b.extend((if frame % 40 < 20 { 1000i16 } else { -1000i16 }).to_le_bytes());
    }
    b
}
fn play(bytes: Vec<u8>, loop_play: bool) -> MixerCommand {
    MixerCommand::Play {
        id: None,
        channel: Channel::Bgm,
        sources: SourcePair {
            base: Arc::new(Bytes(bytes)),
            loop_file: None,
        },
        loop_play,
        gain: 1.0,
        pan: 0.0,
        fade_ms: 0,
    }
}
#[test]
#[ignore = "requires a real native audio output device; emits a short test tone"]
fn native_device_completion_pause_cancel_reopen_and_decode_failure() {
    for _ in 0..3 {
        let stop = Arc::new(AtomicBool::new(false));
        let audio = AudioBridge::start(stop.clone()).unwrap();
        audio.send(play(wave(), false)).unwrap();
        audio.send(MixerCommand::Suspend(true)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(audio.drain_finished().count(), 0);
        audio.send(MixerCommand::Suspend(false)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            audio.check_error().unwrap();
            if audio.drain_finished().next().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        audio.send(play(wave(), true)).unwrap();
        stop.store(true, Ordering::Release);
        audio.close().unwrap();
    }
    let audio = AudioBridge::start(Arc::new(AtomicBool::new(false))).unwrap();
    audio.send(play(b"corrupt".to_vec(), false)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.check_error().is_ok() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(audio.drain_finished().count(), 0);
    assert!(audio.close().is_err());
}
