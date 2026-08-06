use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
};

use astra_emu_family_api::{LegacyVideoCommandV1, LegacyVideoMode};
use astra_emu_fvp::{
    fvp_movie_compatibility, open_fvp_movie_stream, FvpMovieAudioChunk, FvpMovieCompatibility,
    FvpMovieFrame, FvpMoviePacket, FvpMovieStreamDecoder,
};
use astra_media::PlayerDecodedAudio;
use astra_platform::{
    DecodeKind, DecodeOutput, DecodeStreamAction, PlatformDecodeRequest, PlatformHostClient,
};

use crate::audio_executor::HostAudioExecutor;

pub(crate) const MAX_ENCODED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAX_AUDIO_SAMPLES: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 60 * 60 * 4;
const MOVIE_AUDIO_STREAM_BASE: u32 = 0xF000_0000;
const VIDEO_RING_FRAMES: usize = 16;
const VIDEO_PREFETCH_NS: u64 = 500_000_000;

#[derive(Clone)]
pub(crate) struct HostVideoFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba8: Arc<[u8]>,
    pub(crate) stage_width: u32,
    pub(crate) stage_height: u32,
    pub(crate) mode: LegacyVideoMode,
}

struct TimelineFrame {
    pts_ns: u64,
    width: u32,
    height: u32,
    rgba8: Arc<[u8]>,
}

struct ActiveMovie {
    playback_id: String,
    mode: LegacyVideoMode,
    stage_width: u32,
    stage_height: u32,
    frames: VecDeque<TimelineFrame>,
    decoder: MovieDecoder,
    duration_ns: Option<u64>,
    elapsed_ns: u64,
    audio_stream_id: Option<u32>,
    audio_started: bool,
    previous_frame_pts_ns: Option<u64>,
    inferred_frame_step_ns: u64,
    eof: bool,
}

enum MovieDecoder {
    Native(FvpMovieStreamDecoder),
    Platform(Box<PlatformMovieDecoder>),
    #[cfg(test)]
    Buffered(VecDeque<FvpMoviePacket>),
}

struct PlatformMovieDecoder {
    video: PlatformVideoStreamDecoder,
    audio: Option<PlatformAudioStreamDecoder>,
    next_video: Option<FvpMovieFrame>,
    next_audio: Option<FvpMovieAudioChunk>,
    video_eof: bool,
    audio_eof: bool,
}

struct PlatformVideoStreamDecoder {
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    rx: Option<Receiver<Result<PlatformVideoOutput, String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    closed: bool,
    ended: bool,
}

enum PlatformVideoOutput {
    Frame(FvpMovieFrame),
    End,
}

struct PlatformAudioStreamDecoder {
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    pending: Option<FvpMovieAudioChunk>,
    rx: Option<Receiver<Result<PlatformAudioOutput, String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    closed: bool,
    ended: bool,
}

enum PlatformAudioOutput {
    Chunk(FvpMovieAudioChunk),
    End,
}

enum PlatformDecodePoll<T> {
    Pending,
    Item(T),
    End,
}

impl PlatformAudioStreamDecoder {
    fn open(client: PlatformHostClient, codec: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let session = pollster::block_on(client.open_decode(DecodeKind::Audio))
            .map_err(|error| error.to_string())?;
        let output = pollster::block_on(client.decode(
            session,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Audio,
                codec: codec.to_owned(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: DecodeStreamAction::Start,
                bytes,
            },
        ));
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err(error.to_string());
            }
        };
        let mut pending = match Self::parse_chunk(output) {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err(error);
            }
        };
        pending.pts_ms = 0;
        let next_pts_ms = match Self::chunk_duration_ms(&pending) {
            Ok(duration) => duration,
            Err(error) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err(error);
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(VIDEO_RING_FRAMES);
        let worker_stop = Arc::clone(&stop);
        let worker_client = client.clone();
        let worker = match thread::Builder::new()
            .name("astra-rfvp-platform-audio".to_string())
            .spawn(move || {
                platform_audio_worker(worker_client, session, next_pts_ms, worker_stop, tx);
            }) {
            Ok(worker) => worker,
            Err(_) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err("ASTRA_EMU_VIDEO_AUDIO_PLATFORM_WORKER_START".to_owned());
            }
        };
        Ok(Self {
            client,
            session,
            pending: Some(pending),
            rx: Some(rx),
            stop,
            worker: Some(worker),
            closed: false,
            ended: false,
        })
    }

    fn parse_chunk(output: DecodeOutput) -> Result<FvpMovieAudioChunk, String> {
        let DecodeOutput::CpuBuffer { format, bytes, .. } = output else {
            return Err("ASTRA_EMU_VIDEO_AUDIO_PLATFORM_OUTPUT_KIND".to_owned());
        };
        let parsed = PlayerDecodedAudio::parse(&format, &bytes, bytes.len() / 2)
            .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_OUTPUT_INVALID".to_owned())?;
        Ok(FvpMovieAudioChunk {
            pts_ms: 0,
            sample_rate: parsed.sample_rate,
            channels: parsed.channels,
            samples: parsed.samples,
        })
    }

    fn poll_next_chunk(&mut self) -> Result<PlatformDecodePoll<FvpMovieAudioChunk>, String> {
        if let Some(chunk) = self.pending.take() {
            return Ok(PlatformDecodePoll::Item(chunk));
        }
        if self.ended {
            return Ok(PlatformDecodePoll::End);
        }
        let output = match self
            .rx
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_VIDEO_AUDIO_PLATFORM_CLOSED".to_owned())?
            .try_recv()
        {
            Ok(output) => output,
            Err(mpsc::TryRecvError::Empty) => return Ok(PlatformDecodePoll::Pending),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("ASTRA_EMU_VIDEO_AUDIO_PLATFORM_WORKER_CLOSED".to_owned())
            }
        }?;
        match output {
            PlatformAudioOutput::Chunk(chunk) => Ok(PlatformDecodePoll::Item(chunk)),
            PlatformAudioOutput::End => {
                self.ended = true;
                Ok(PlatformDecodePoll::End)
            }
        }
    }

    fn chunk_duration_ms(chunk: &FvpMovieAudioChunk) -> Result<u64, String> {
        let frames = chunk
            .samples
            .len()
            .checked_div(usize::from(chunk.channels))
            .ok_or_else(|| "ASTRA_EMU_VIDEO_AUDIO_CHANNEL_BOUNDS".to_owned())?;
        u64::try_from(frames)
            .ok()
            .and_then(|frames| {
                frames
                    .checked_mul(1_000)
                    .and_then(|value| value.checked_div(u64::from(chunk.sample_rate)))
            })
            .map(|duration| duration.max(1))
            .ok_or_else(|| "ASTRA_EMU_VIDEO_AUDIO_TIMELINE_BOUNDS".to_owned())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.stop.store(true, Ordering::Release);
        self.rx.take();
        let worker_result = self.worker.take().map_or(Ok(()), |worker| {
            worker
                .join()
                .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_PLATFORM_WORKER_PANIC".to_owned())
        });
        let close_result = pollster::block_on(self.client.close_decode(self.session))
            .map_err(|error| error.to_string());
        self.closed = true;
        worker_result.and(close_result)
    }

    fn close(mut self) -> Result<(), String> {
        self.shutdown()
    }
}

impl Drop for PlatformAudioStreamDecoder {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn platform_audio_worker(
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    mut next_pts_ms: u64,
    stop: Arc<AtomicBool>,
    tx: SyncSender<Result<PlatformAudioOutput, String>>,
) {
    let mut sequence = 2_u64;
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let output = match pollster::block_on(client.decode(
            session,
            PlatformDecodeRequest {
                sequence,
                kind: DecodeKind::Audio,
                codec: String::new(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: false,
                stream_action: DecodeStreamAction::Next,
                bytes: Vec::new(),
            },
        )) {
            Ok(output) => output,
            Err(error)
                if error.operation == "decode.stream.next"
                    && error.fields.get("diagnostic_code").is_some_and(|code| {
                        code == astra_platform::DECODE_STREAM_EOS_DIAGNOSTIC
                    }) =>
            {
                send_platform_audio_result(&tx, &stop, Ok(PlatformAudioOutput::End));
                return;
            }
            Err(error) => {
                send_platform_audio_result(&tx, &stop, Err(error.to_string()));
                return;
            }
        };
        let mut chunk = match PlatformAudioStreamDecoder::parse_chunk(output) {
            Ok(chunk) => chunk,
            Err(error) => {
                send_platform_audio_result(&tx, &stop, Err(error));
                return;
            }
        };
        chunk.pts_ms = next_pts_ms;
        let duration = match PlatformAudioStreamDecoder::chunk_duration_ms(&chunk) {
            Ok(duration) => duration,
            Err(error) => {
                send_platform_audio_result(&tx, &stop, Err(error));
                return;
            }
        };
        next_pts_ms = match next_pts_ms.checked_add(duration) {
            Some(next_pts_ms) => next_pts_ms,
            None => {
                send_platform_audio_result(
                    &tx,
                    &stop,
                    Err("ASTRA_EMU_VIDEO_AUDIO_TIMELINE_BOUNDS".to_owned()),
                );
                return;
            }
        };
        send_platform_audio_result(&tx, &stop, Ok(PlatformAudioOutput::Chunk(chunk)));
        sequence = match sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                send_platform_audio_result(
                    &tx,
                    &stop,
                    Err("ASTRA_EMU_VIDEO_AUDIO_PLATFORM_SEQUENCE".to_owned()),
                );
                return;
            }
        };
    }
}

fn send_platform_audio_result(
    tx: &SyncSender<Result<PlatformAudioOutput, String>>,
    stop: &AtomicBool,
    result: Result<PlatformAudioOutput, String>,
) {
    if stop.load(Ordering::Acquire) {
        return;
    }
    // A full ring is backpressure, not a reason to spin.  The receiver is
    // dropped during shutdown, which releases this blocking send with an
    // error and lets the worker terminate without a polling loop.
    let _ = tx.send(result);
}

impl PlatformVideoStreamDecoder {
    fn open(client: PlatformHostClient, codec: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let session = pollster::block_on(client.open_decode(DecodeKind::Video))
            .map_err(|error| error.to_string())?;
        let start = pollster::block_on(client.decode(
            session,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: codec.to_owned(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                stream_action: DecodeStreamAction::Start,
                bytes,
            },
        ));
        let output = match start {
            Ok(output) => output,
            Err(error) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err(error.to_string());
            }
        };
        let DecodeOutput::CpuBuffer { format, bytes, .. } = output else {
            let _ = pollster::block_on(client.close_decode(session));
            return Err("ASTRA_EMU_VIDEO_PLATFORM_DESCRIPTOR_KIND".into());
        };
        if format
            != format!(
                "postcard:{}",
                astra_media::DECODED_VIDEO_STREAM_CURSOR_SCHEMA
            )
        {
            let _ = pollster::block_on(client.close_decode(session));
            return Err("ASTRA_EMU_VIDEO_PLATFORM_DESCRIPTOR_FORMAT".into());
        }
        let cursor = match astra_media::DecodedVideoStreamCursor::decode(&bytes) {
            Ok(cursor) => cursor,
            Err(error) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err(error.to_string());
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(VIDEO_RING_FRAMES);
        let worker_stop = Arc::clone(&stop);
        let worker_client = client.clone();
        let worker_cursor = cursor.clone();
        let worker = match thread::Builder::new()
            .name("astra-rfvp-platform-video".to_string())
            .spawn(move || {
                platform_video_worker(worker_client, session, worker_cursor, worker_stop, tx);
            }) {
            Ok(worker) => worker,
            Err(_) => {
                let _ = pollster::block_on(client.close_decode(session));
                return Err("ASTRA_EMU_VIDEO_PLATFORM_WORKER_START".to_owned());
            }
        };
        Ok(Self {
            client,
            session,
            rx: Some(rx),
            stop,
            worker: Some(worker),
            closed: false,
            ended: false,
        })
    }

    fn poll_next_frame(&mut self) -> Result<PlatformDecodePoll<FvpMovieFrame>, String> {
        if self.ended {
            return Ok(PlatformDecodePoll::End);
        }
        let output = match self
            .rx
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_VIDEO_PLATFORM_CLOSED".to_owned())?
            .try_recv()
        {
            Ok(output) => output,
            Err(mpsc::TryRecvError::Empty) => return Ok(PlatformDecodePoll::Pending),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("ASTRA_EMU_VIDEO_PLATFORM_WORKER_CLOSED".to_owned())
            }
        }?;
        match output {
            PlatformVideoOutput::Frame(frame) => Ok(PlatformDecodePoll::Item(frame)),
            PlatformVideoOutput::End => {
                self.ended = true;
                Ok(PlatformDecodePoll::End)
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.stop.store(true, Ordering::Release);
        self.rx.take();
        let worker_result = self.worker.take().map_or(Ok(()), |worker| {
            worker
                .join()
                .map_err(|_| "ASTRA_EMU_VIDEO_PLATFORM_WORKER_PANIC".to_owned())
        });
        let close_result = pollster::block_on(self.client.close_decode(self.session))
            .map_err(|error| error.to_string());
        self.closed = true;
        worker_result.and(close_result)
    }

    fn close(mut self) -> Result<(), String> {
        self.shutdown()
    }
}

impl Drop for PlatformVideoStreamDecoder {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn platform_video_worker(
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    cursor: astra_media::DecodedVideoStreamCursor,
    stop: Arc<AtomicBool>,
    tx: SyncSender<Result<PlatformVideoOutput, String>>,
) {
    let mut sequence = 2_u64;
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let output = match pollster::block_on(client.decode(
            session,
            PlatformDecodeRequest {
                sequence,
                kind: DecodeKind::Video,
                codec: String::new(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: false,
                stream_action: DecodeStreamAction::Next,
                bytes: Vec::new(),
            },
        )) {
            Ok(output) => output,
            Err(error) => {
                send_platform_video_result(&tx, &stop, Err(error.to_string()));
                return;
            }
        };
        let parsed = parse_platform_video_output(output, &cursor);
        let is_end = matches!(parsed, Ok(PlatformVideoOutput::End));
        send_platform_video_result(&tx, &stop, parsed);
        if is_end {
            return;
        }
        sequence = match sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                send_platform_video_result(
                    &tx,
                    &stop,
                    Err("ASTRA_EMU_VIDEO_PLATFORM_SEQUENCE".to_owned()),
                );
                return;
            }
        };
    }
}

fn send_platform_video_result(
    tx: &SyncSender<Result<PlatformVideoOutput, String>>,
    stop: &AtomicBool,
    result: Result<PlatformVideoOutput, String>,
) {
    if stop.load(Ordering::Acquire) {
        return;
    }
    let _ = tx.send(result);
}

fn parse_platform_video_output(
    output: DecodeOutput,
    cursor: &astra_media::DecodedVideoStreamCursor,
) -> Result<PlatformVideoOutput, String> {
    let DecodeOutput::CpuBuffer {
        format,
        bytes,
        hash,
    } = output
    else {
        return Err("ASTRA_EMU_VIDEO_PLATFORM_FRAME_KIND".to_owned());
    };
    let frame = if format == format!("postcard:{}", astra_media::DECODED_VIDEO_FRAME_SCHEMA) {
        astra_media::DecodedVideoFrame::decode(&bytes, MAX_DECODED_BYTES as u64)
            .map_err(|error| error.to_string())?
    } else if astra_media::is_decoded_video_cpu_buffer_format(&format) {
        astra_media::DecodedVideoFrame::from_cpu_buffer(
            &format,
            bytes,
            &hash,
            MAX_DECODED_BYTES as u64,
        )
        .map_err(|error| error.to_string())?
    } else if format
        == format!(
            "postcard:{}",
            astra_media::DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA
        )
    {
        let end: astra_media::DecodedVideoStreamCursorEnd = postcard::from_bytes(&bytes)
            .map_err(|error| format!("ASTRA_EMU_VIDEO_PLATFORM_END_DECODE:{error}"))?;
        end.validate_against(cursor)
            .map_err(|error| error.to_string())?;
        return Ok(PlatformVideoOutput::End);
    } else {
        return Err("ASTRA_EMU_VIDEO_PLATFORM_FRAME_FORMAT".to_owned());
    };
    {
        let mut rgba8 = frame.bgra8;
        for pixel in rgba8.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Ok(PlatformVideoOutput::Frame(FvpMovieFrame {
            pts_ms: frame.pts_us / 1_000,
            width: frame.width,
            height: frame.height,
            rgba8,
        }))
    }
}

impl MovieDecoder {
    fn close(self) -> Result<(), String> {
        match self {
            Self::Platform(mut decoder) => {
                let video_result = decoder.video.close();
                let audio_result = decoder
                    .audio
                    .take()
                    .map_or(Ok(()), PlatformAudioStreamDecoder::close);
                video_result.and(audio_result)
            }
            Self::Native(_) => Ok(()),
            #[cfg(test)]
            Self::Buffered(_) => Ok(()),
        }
    }

    fn next_packet(&mut self) -> Result<Option<FvpMoviePacket>, String> {
        match self {
            Self::Native(decoder) => decoder
                .next_packet()
                .map(Some)
                .map_err(|error| error.to_string()),
            Self::Platform(decoder) => {
                if decoder.next_video.is_none() && !decoder.video_eof {
                    match decoder
                        .video
                        .poll_next_frame()
                        .map_err(|_| "ASTRA_EMU_VIDEO_PLATFORM_DECODE_FAILED".to_owned())?
                    {
                        PlatformDecodePoll::Pending => {}
                        PlatformDecodePoll::Item(frame) => decoder.next_video = Some(frame),
                        PlatformDecodePoll::End => decoder.video_eof = true,
                    }
                }
                if decoder.next_audio.is_none() && !decoder.audio_eof {
                    match decoder.audio.as_mut() {
                        Some(audio) => match audio
                            .poll_next_chunk()
                            .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_DECODE_FAILED".to_owned())?
                        {
                            PlatformDecodePoll::Pending => {}
                            PlatformDecodePoll::Item(chunk) => decoder.next_audio = Some(chunk),
                            PlatformDecodePoll::End => decoder.audio_eof = true,
                        },
                        None => decoder.audio_eof = true,
                    }
                }
                let take_audio = match (decoder.next_audio.as_ref(), decoder.next_video.as_ref()) {
                    (Some(audio), Some(video)) => audio.pts_ms <= video.pts_ms,
                    (Some(_), None) => true,
                    _ => false,
                };
                if take_audio {
                    Ok(decoder.next_audio.take().map(FvpMoviePacket::Audio))
                } else if let Some(frame) = decoder.next_video.take() {
                    Ok(Some(FvpMoviePacket::Video(frame)))
                } else if decoder.video_eof && decoder.audio_eof {
                    Ok(Some(FvpMoviePacket::End))
                } else {
                    Ok(None)
                }
            }
            #[cfg(test)]
            Self::Buffered(packets) => Ok(Some(packets.pop_front().unwrap_or(FvpMoviePacket::End))),
        }
    }
}

#[derive(Default)]
pub(crate) struct HostVideoExecutor {
    active: Option<ActiveMovie>,
    completed: Vec<String>,
    audio_sequence: u32,
    platform: Option<PlatformHostClient>,
}

impl HostVideoExecutor {
    pub(crate) fn bind_platform(&mut self, platform: PlatformHostClient) {
        self.platform = Some(platform);
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn execute(
        &mut self,
        command: LegacyVideoCommandV1,
        resolved_resource: Option<Vec<u8>>,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        match command {
            LegacyVideoCommandV1::Play {
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
            } => self.play(
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
                resolved_resource.ok_or_else(|| "ASTRA_EMU_VIDEO_RESOURCE_MISSING".to_owned())?,
                audio,
            ),
            LegacyVideoCommandV1::Stop { playback_id } => self.stop(&playback_id, audio, true),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn play(
        &mut self,
        playback_id: String,
        resource_uri: String,
        mode: LegacyVideoMode,
        stage_width: u32,
        stage_height: u32,
        bytes: Vec<u8>,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_VIDEO_PLAYBACK_ALREADY_ACTIVE".into());
        }
        let extension = resource_uri
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .ok_or_else(|| "ASTRA_EMU_VIDEO_EXTENSION_MISSING".to_owned())?;
        let compatibility = fvp_movie_compatibility(extension);
        match compatibility {
            FvpMovieCompatibility::Native | FvpMovieCompatibility::PlatformProviderRequired => {}
            FvpMovieCompatibility::Unsupported => {
                return Err("ASTRA_EMU_VIDEO_CODEC_UNSUPPORTED".into());
            }
        }
        let (decoder, duration_ns) = match compatibility {
            FvpMovieCompatibility::Native => (
                MovieDecoder::Native(
                    open_fvp_movie_stream(
                        extension,
                        Arc::from(bytes),
                        MAX_FRAMES,
                        MAX_DECODED_BYTES,
                        MAX_AUDIO_SAMPLES,
                    )
                    .map_err(|error| error.to_string())?,
                ),
                None,
            ),
            FvpMovieCompatibility::PlatformProviderRequired => {
                let platform = self
                    .platform
                    .clone()
                    .ok_or_else(|| "ASTRA_EMU_VIDEO_PLATFORM_HOST_MISSING".to_owned())?;
                // A single encoded source hand-off is moved into the video session.  Only
                // when a separate audio session is required do we make one bounded source
                // clone; the previous path cloned the complete source for both sessions.
                let audio_bytes = if matches!(mode, LegacyVideoMode::ModalWithAudio) {
                    Some(bytes.clone())
                } else {
                    None
                };
                let video = PlatformVideoStreamDecoder::open(platform.clone(), extension, bytes)?;
                let audio = if matches!(mode, LegacyVideoMode::ModalWithAudio) {
                    match PlatformAudioStreamDecoder::open(
                        platform,
                        extension,
                        audio_bytes
                            .ok_or_else(|| "ASTRA_EMU_VIDEO_AUDIO_SOURCE_MISSING".to_owned())?,
                    ) {
                        Ok(audio) => Some(audio),
                        Err(error) => {
                            let _ = video.close();
                            return Err(error);
                        }
                    }
                } else {
                    None
                };
                (
                    MovieDecoder::Platform(Box::new(PlatformMovieDecoder {
                        video,
                        audio,
                        next_video: None,
                        next_audio: None,
                        video_eof: false,
                        audio_eof: false,
                    })),
                    None,
                )
            }
            FvpMovieCompatibility::Unsupported => unreachable!(),
        };
        let audio_stream_id = if matches!(mode, LegacyVideoMode::ModalWithAudio) {
            let stream_id = MOVIE_AUDIO_STREAM_BASE
                .checked_add(self.audio_sequence)
                .ok_or_else(|| "ASTRA_EMU_MOVIE_AUDIO_ID_BOUNDS".to_owned())?;
            self.audio_sequence = self.audio_sequence.saturating_add(1);
            Some(stream_id)
        } else {
            None
        };
        let mut active = ActiveMovie {
            playback_id,
            mode,
            stage_width,
            stage_height,
            frames: VecDeque::new(),
            decoder,
            duration_ns,
            elapsed_ns: 0,
            audio_stream_id,
            audio_started: false,
            previous_frame_pts_ns: None,
            inferred_frame_step_ns: 34_000_000,
            eof: false,
        };
        pump_decoder(&mut active, audio)?;
        if active.frames.is_empty() && active.eof {
            return Err("ASTRA_EMU_VIDEO_DECODE_NO_FRAME".into());
        }
        self.active = Some(active);
        Ok(())
    }

    pub(crate) fn advance(
        &mut self,
        delta_ns: u64,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.elapsed_ns = active
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or_else(|| "ASTRA_EMU_VIDEO_TIMELINE_BOUNDS".to_owned())?;
        pump_decoder(active, audio)?;
        while active.frames.len() > 1
            && active
                .frames
                .get(1)
                .is_some_and(|frame| frame.pts_ns <= active.elapsed_ns)
        {
            active.frames.pop_front();
        }
        pump_decoder(active, audio)?;
        if active
            .duration_ns
            .is_some_and(|duration| active.eof && active.elapsed_ns >= duration)
        {
            let playback_id = active.playback_id.clone();
            self.stop(&playback_id, audio, true)?;
        }
        Ok(())
    }

    fn stop(
        &mut self,
        playback_id: &str,
        audio: &mut HostAudioExecutor,
        complete: bool,
    ) -> Result<(), String> {
        let active = self
            .active
            .take()
            .ok_or_else(|| "ASTRA_EMU_VIDEO_PLAYBACK_MISSING".to_owned())?;
        if active.playback_id != playback_id {
            self.active = Some(active);
            return Err("ASTRA_EMU_VIDEO_PLAYBACK_IDENTITY".into());
        }
        if let Some(stream_id) = active.audio_stream_id.filter(|_| active.audio_started) {
            audio.stop_movie_pcm(stream_id)?;
        }
        active.decoder.close()?;
        if complete {
            self.completed.push(playback_id.to_owned());
        }
        Ok(())
    }

    pub(crate) fn current_frame(&self) -> Option<HostVideoFrame> {
        let active = self.active.as_ref()?;
        let frame = active.frames.front()?;
        Some(HostVideoFrame {
            width: frame.width,
            height: frame.height,
            rgba8: Arc::clone(&frame.rgba8),
            stage_width: active.stage_width,
            stage_height: active.stage_height,
            mode: active.mode,
        })
    }

    pub(crate) fn take_completed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.completed)
    }

    pub(crate) fn reset(&mut self, audio: &mut HostAudioExecutor) -> Result<(), String> {
        if let Some(active) = self.active.take() {
            if let Some(stream_id) = active.audio_stream_id.filter(|_| active.audio_started) {
                audio.stop_movie_pcm(stream_id)?;
            }
            active.decoder.close()?;
        }
        self.completed.clear();
        Ok(())
    }
}

fn pump_decoder(active: &mut ActiveMovie, audio: &mut HostAudioExecutor) -> Result<(), String> {
    while !active.eof
        && active.frames.len() < VIDEO_RING_FRAMES
        && active
            .frames
            .back()
            .is_none_or(|frame| frame.pts_ns <= active.elapsed_ns.saturating_add(VIDEO_PREFETCH_NS))
    {
        let Some(packet) = active.decoder.next_packet()? else {
            // PlatformHost decode is producer-driven.  A slow hardware
            // decoder must not block the Runtime tick waiting for the bounded
            // ring; the next deadline/event will pump it again.
            break;
        };
        match packet {
            FvpMoviePacket::Video(frame) => {
                let pts_ns = frame
                    .pts_ms
                    .checked_mul(1_000_000)
                    .ok_or_else(|| "ASTRA_EMU_VIDEO_TIMELINE_BOUNDS".to_owned())?;
                if let Some(previous) = active.previous_frame_pts_ns {
                    if pts_ns < previous {
                        return Err("ASTRA_EMU_VIDEO_TIMELINE_ORDER".into());
                    }
                    if pts_ns > previous {
                        active.inferred_frame_step_ns = pts_ns - previous;
                    }
                }
                active.previous_frame_pts_ns = Some(pts_ns);
                active.frames.push_back(TimelineFrame {
                    pts_ns,
                    width: frame.width,
                    height: frame.height,
                    rgba8: Arc::from(frame.rgba8),
                });
            }
            FvpMoviePacket::Audio(chunk) => {
                if let Some(stream_id) = active.audio_stream_id {
                    if active.audio_started {
                        audio.append_movie_stream(
                            stream_id,
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.samples,
                        )?;
                    } else {
                        audio.begin_movie_stream(
                            stream_id,
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.samples,
                        )?;
                        active.audio_started = true;
                    }
                }
            }
            FvpMoviePacket::End => {
                active.eof = true;
                if active.duration_ns.is_none() {
                    active.duration_ns = active
                        .previous_frame_pts_ns
                        .and_then(|pts| pts.checked_add(active.inferred_frame_step_ns));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_frame_tracks_fixed_timeline_without_wall_clock() {
        let mut executor = HostVideoExecutor {
            active: Some(ActiveMovie {
                playback_id: "movie.1".into(),
                mode: LegacyVideoMode::LayerNoAudio,
                stage_width: 1280,
                stage_height: 720,
                frames: VecDeque::from(vec![
                    TimelineFrame {
                        pts_ns: 0,
                        width: 1,
                        height: 1,
                        rgba8: Arc::from([1, 2, 3, 4]),
                    },
                    TimelineFrame {
                        pts_ns: 20,
                        width: 1,
                        height: 1,
                        rgba8: Arc::from([5, 6, 7, 8]),
                    },
                ]),
                decoder: MovieDecoder::Buffered(VecDeque::new()),
                duration_ns: Some(40),
                elapsed_ns: 0,
                audio_stream_id: None,
                audio_started: false,
                previous_frame_pts_ns: Some(20),
                inferred_frame_step_ns: 20,
                eof: true,
            }),
            ..Default::default()
        };
        executor.active.as_mut().unwrap().elapsed_ns = 20;
        while executor.active.as_ref().unwrap().frames.len() > 1
            && executor.active.as_ref().unwrap().frames[1].pts_ns
                <= executor.active.as_ref().unwrap().elapsed_ns
        {
            executor.active.as_mut().unwrap().frames.pop_front();
        }
        assert_eq!(&*executor.current_frame().unwrap().rgba8, &[5, 6, 7, 8]);
    }

    #[test]
    fn raw_platform_video_frame_moves_payload_into_rgba_without_postcard() {
        let bgra = vec![1, 2, 3, 255];
        let payload_ptr = bgra.as_ptr();
        let frame = astra_media::DecodedVideoFrame {
            sequence: 1,
            pts_us: 0,
            duration_us: 10_000,
            width: 1,
            height: 1,
            content_hash: astra_core::Hash256::from_sha256(&bgra),
            bgra8: bgra,
        };
        let cursor = astra_media::DecodedVideoStreamCursor {
            schema: astra_media::DECODED_VIDEO_STREAM_CURSOR_SCHEMA.into(),
            source_hash: astra_core::Hash256::from_sha256(b"source"),
            width: 1,
            height: 1,
            max_frames: 4,
            max_decoded_byte_count: 64,
        };
        let output = DecodeOutput::CpuBuffer {
            format: frame.cpu_buffer_format(),
            hash: frame.content_hash.to_string(),
            bytes: frame.bgra8,
        };
        let PlatformVideoOutput::Frame(frame) =
            parse_platform_video_output(output, &cursor).unwrap()
        else {
            panic!("raw platform frame was not returned");
        };
        assert_eq!(frame.rgba8.as_ptr(), payload_ptr);
        assert_eq!(&*frame.rgba8, &[3, 2, 1, 255]);
    }
}
