//! Thread-owned incremental FFmpeg decode, independent of Engine/VN sessions.
//! One outstanding operation and a bounded packet batch prevent decoded payload accumulation.
use crate::CoreError;
use astra_media::FfmpegPlaybackDecoder;
pub use astra_media::{
    AudioFramePacket, DecodedMediaPacket, FfmpegAudioOutputFormat, FfmpegStreamLimits,
    MediaPlaybackConfig, VideoFramePacket,
};
mod pcm;
pub use pcm::{PcmMixResult, PcmQueue};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;

enum Command {
    Next { packets: usize, bytes: usize },
    Seek(u64),
}
#[derive(Debug)]
pub enum DecodeCompletion {
    Opened(MediaPlaybackConfig),
    Packets {
        packets: Vec<DecodedMediaPacket>,
        eof: bool,
    },
    Seeked {
        generation: u64,
    },
}

pub struct VideoDecoderWorker {
    commands: Option<mpsc::SyncSender<Command>>,
    results: Option<mpsc::Receiver<Result<DecodeCompletion, CoreError>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), CoreError>>>,
    pending: bool,
}
impl VideoDecoderWorker {
    /// Opening is asynchronous. Poll Opened before requesting a packet or seek.
    pub fn open(
        codec: String,
        bytes: Arc<[u8]>,
        limits: FfmpegStreamLimits,
        audio: Option<FfmpegAudioOutputFormat>,
    ) -> Result<Self, CoreError> {
        let (commands, requests) = mpsc::sync_channel(1);
        let (replies, results) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let worker = std::thread::Builder::new()
            .name("astra-emu-video".into())
            .spawn(move || run(codec, bytes, limits, audio, requests, replies, cancelled))
            .map_err(|cause| CoreError::invalid("ASTRA_EMU_VIDEO_THREAD", cause.to_string()))?;
        Ok(Self {
            commands: Some(commands),
            results: Some(results),
            stop,
            worker: Some(worker),
            pending: true,
        })
    }
    /// Decode a bounded batch so packet throughput is independent of host frame rate.
    /// A packet larger than the byte budget fails rather than exceeding the budget.
    pub fn request_next(&mut self, packets: usize, bytes: usize) -> Result<(), CoreError> {
        if !(1..=16).contains(&packets) || bytes == 0 || bytes > 256 * 1024 * 1024 {
            return Err(CoreError::invalid(
                "ASTRA_EMU_VIDEO_BATCH",
                "invalid decode batch budget",
            ));
        }
        self.submit(Command::Next { packets, bytes })
    }
    /// Seek is serialized with decode; its generation is carried by subsequent packets.
    pub fn request_seek(&mut self, position_us: u64) -> Result<(), CoreError> {
        self.submit(Command::Seek(position_us))
    }
    fn submit(&mut self, request: Command) -> Result<(), CoreError> {
        if self.pending || self.stop.load(Ordering::Acquire) {
            return Err(CoreError::invalid(
                "ASTRA_EMU_VIDEO_STATE",
                "decoder is busy or closed",
            ));
        }
        self.commands
            .as_ref()
            .ok_or_else(closed)?
            .try_send(request)
            .map_err(|_| closed())?;
        self.pending = true;
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<DecodeCompletion>, CoreError> {
        if !self.pending {
            return Ok(None);
        }
        match self.results.as_ref().ok_or_else(closed)?.try_recv() {
            Ok(result) => {
                self.pending = false;
                if result.is_err() {
                    self.stop.store(true, Ordering::Release);
                }
                result.map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(closed()),
        }
    }
    /// Cancel outstanding work, discard queued/late results, and wait for native release.
    pub fn close(mut self) -> Result<(), CoreError> {
        self.shutdown()
    }
    fn shutdown(&mut self) -> Result<(), CoreError> {
        self.stop.store(true, Ordering::Release);
        self.commands.take();
        self.results.take();
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| {
                CoreError::invalid("ASTRA_EMU_VIDEO_PANIC", "decoder worker panicked")
            })??;
        }
        Ok(())
    }
}
impl Drop for VideoDecoderWorker {
    fn drop(&mut self) {
        if let Err(cause) = self.shutdown() {
            tracing::error!(
                event = "astra.emu.video.shutdown_failed",
                code = cause.code()
            );
        }
    }
}
fn closed() -> CoreError {
    CoreError::invalid("ASTRA_EMU_VIDEO_CLOSED", "decoder channel is closed")
}
fn decode_error(cause: astra_media::MediaError) -> CoreError {
    CoreError::invalid("ASTRA_EMU_VIDEO_DECODE", cause.to_string())
}
fn run(
    codec: String,
    bytes: Arc<[u8]>,
    limits: FfmpegStreamLimits,
    audio: Option<FfmpegAudioOutputFormat>,
    requests: mpsc::Receiver<Command>,
    replies: mpsc::SyncSender<Result<DecodeCompletion, CoreError>>,
    stop: Arc<AtomicBool>,
) -> Result<(), CoreError> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut decoder =
        match FfmpegPlaybackDecoder::open_with_audio_output(&codec, &bytes, limits, audio) {
            Ok(decoder) => decoder,
            Err(cause) => {
                let _ = replies.send(Err(decode_error(cause)));
                return Ok(());
            }
        };
    let mut carried = None;
    if !stop.load(Ordering::Acquire)
        && replies
            .send(Ok(DecodeCompletion::Opened(decoder.playback_config())))
            .is_ok()
    {
        while let Ok(request) = requests.recv() {
            if stop.load(Ordering::Acquire) {
                break;
            }
            let result = match request {
                Command::Next { packets, bytes } => {
                    read_batch(&mut decoder, &mut carried, packets, bytes, &stop)
                }
                Command::Seek(position) => {
                    carried = None;
                    decoder
                        .seek(position)
                        .map(|generation| DecodeCompletion::Seeked { generation })
                        .map_err(decode_error)
                }
            };
            let failed = result.is_err();
            if stop.load(Ordering::Acquire) || replies.send(result).is_err() || failed {
                break;
            }
        }
    }
    decoder.cancel().map_err(decode_error)
}

fn read_batch(
    decoder: &mut FfmpegPlaybackDecoder,
    carried: &mut Option<DecodedMediaPacket>,
    max_packets: usize,
    max_bytes: usize,
    stop: &AtomicBool,
) -> Result<DecodeCompletion, CoreError> {
    let mut packets = Vec::with_capacity(max_packets);
    let mut bytes = 0;
    let mut eof = false;
    while packets.len() < max_packets && !stop.load(Ordering::Acquire) {
        let next = match carried.take() {
            Some(packet) => Some(packet),
            None => decoder.read_next().map_err(decode_error)?,
        };
        let Some(packet) = next else {
            eof = true;
            break;
        };
        let size = match &packet {
            DecodedMediaPacket::Video { bgra8, .. } => bgra8.len(),
            DecodedMediaPacket::Audio { samples, .. } => std::mem::size_of_val(samples.as_slice()),
        };
        if size > max_bytes {
            return Err(CoreError::invalid(
                "ASTRA_EMU_VIDEO_BATCH",
                "decoded packet exceeds batch byte budget",
            ));
        }
        if size > max_bytes - bytes {
            *carried = Some(packet);
            break;
        }
        bytes += size;
        packets.push(packet);
    }
    Ok(DecodeCompletion::Packets { packets, eof })
}
