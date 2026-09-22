use super::*;
use abi_stable::{std_types::RResult, type_level::downcasting::TD_Opaque};
use astra_emu_family_api::{AudioSink, AudioSink_TO, FfiFamilyResult};
use std::sync::{mpsc, Condvar};
use std::time::Duration;
struct BlockSink {
    shared: Arc<(Mutex<bool>, Condvar)>,
    entered: mpsc::SyncSender<()>,
}
impl AudioSink for BlockSink {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()> {
        assert_eq!(format, OUTPUT_FORMAT);
        RResult::ROk(())
    }
    fn write(&self, chunk: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
        chunk.validate(OUTPUT_FORMAT).unwrap();
        let _ = self.entered.try_send(());
        let (lock, wake) = &*self.shared;
        let mut cancelled = lock.lock().unwrap();
        while !*cancelled {
            cancelled = wake.wait(cancelled).unwrap();
        }
        RResult::ROk(AudioWriteStatus::Cancelled)
    }
    fn is_cancelled(&self) -> bool {
        *self.shared.0.lock().unwrap()
    }
    fn cancel(&self) -> FfiFamilyResult<()> {
        *self.shared.0.lock().unwrap() = true;
        self.shared.1.notify_all();
        RResult::ROk(())
    }
}
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
    b.extend(36_u32.to_le_bytes().map(|_| 0));
    b[4..8].copy_from_slice(&(36u32 + 16000).to_le_bytes());
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
    for _ in 0..8000 {
        b.extend(1000i16.to_le_bytes());
    }
    b
}
fn setup() -> (AudioBridge, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let sink = AudioSink_TO::from_value(
        BlockSink {
            shared: Arc::new((Mutex::new(false), Condvar::new())),
            entered: tx,
        },
        TD_Opaque,
    );
    (AudioBridge::start(sink).unwrap(), rx)
}
fn play(bytes: Vec<u8>) -> MixerCommand {
    MixerCommand::Play {
        id: None,
        channel: Channel::Bgm,
        sources: SourcePair {
            base: Arc::new(Bytes(bytes)),
            loop_file: None,
        },
        loop_play: true,
        gain: 1.0,
        pan: 0.0,
        fade_ms: 0,
    }
}
#[test]
fn close_and_drop_cancel_blocked_pcm_then_join_on_repeated_open() {
    for drop_only in [false, true, false] {
        let (audio, entered) = setup();
        audio.send(play(wave())).unwrap();
        entered
            .recv_timeout(Duration::from_secs(5))
            .expect("worker reached bounded PCM sink");
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            if drop_only {
                drop(audio)
            } else {
                audio.close().unwrap()
            }
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("close joined audio worker");
    }
}
#[test]
fn corrupt_audio_reports_failure_without_pcm_or_completion() {
    let (audio, entered) = setup();
    audio.send(play(b"corrupt".to_vec())).unwrap();
    for _ in 0..1000 {
        if audio.check_error().is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(audio.check_error().is_err());
    assert!(audio.drain_finished().is_empty());
    assert!(entered.try_recv().is_err());
    assert!(audio.close().is_err());
}
#[test]
fn idle_worker_is_joined() {
    let (audio, _) = setup();
    audio.close().unwrap();
}
