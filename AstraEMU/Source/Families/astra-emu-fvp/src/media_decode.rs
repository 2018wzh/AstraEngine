use std::{collections::VecDeque, io::Cursor, sync::Arc};

use na_mpeg2_decoder::{MpegAvEvent, MpegAvPipeline};
use thiserror::Error;
use wmv_decoder::{AsfWmaDecoder, AsfWmv2Decoder, YuvFrame};

#[derive(Debug, Clone)]
pub struct FvpDecodedMovie {
    pub frames: Vec<FvpMovieFrame>,
    pub audio: Option<FvpMovieAudio>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FvpMovieFrame {
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FvpMovieAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct FvpMovieAudioChunk {
    pub pts_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum FvpMoviePacket {
    Video(FvpMovieFrame),
    Audio(FvpMovieAudioChunk),
    End,
}

pub struct FvpMovieStreamDecoder {
    inner: StreamDecoder,
    max_frames: usize,
    max_decoded_bytes: usize,
    max_audio_samples: usize,
    frame_count: usize,
    decoded_bytes: usize,
    audio_samples: usize,
    previous_video_pts: Option<u64>,
    audio_format: Option<(u32, u16)>,
    ended: bool,
}

enum StreamDecoder {
    Wmv(Box<WmvStreamDecoder>),
    Mpeg(Box<MpegStreamDecoder>),
}

struct WmvStreamDecoder {
    video: AsfWmv2Decoder<Cursor<Arc<[u8]>>>,
    audio: Option<AsfWmaDecoder<Cursor<Arc<[u8]>>>>,
    pending_video: Option<FvpMovieFrame>,
    pending_audio: Option<FvpMovieAudioChunk>,
    video_eof: bool,
    audio_eof: bool,
}

struct MpegStreamDecoder {
    pipeline: MpegAvPipeline,
    bytes: Arc<[u8]>,
    offset: usize,
    pending: VecDeque<MpegAvEvent>,
    flushed: bool,
}

const MPEG_INPUT_CHUNK_BYTES: usize = 64 * 1024;
const MAX_PENDING_PACKETS: usize = 256;

#[derive(Debug, Error)]
pub enum FvpMovieDecodeError {
    #[error("ASTRA_FVP_MOVIE_CODEC_UNSUPPORTED")]
    Unsupported,
    #[error("ASTRA_FVP_MOVIE_DECODE")]
    Decode,
    #[error("ASTRA_FVP_MOVIE_BUDGET")]
    Budget,
    #[error("ASTRA_FVP_MOVIE_TIMELINE")]
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FvpMovieCompatibility {
    Native,
    PlatformProviderRequired,
    Unsupported,
}

pub fn fvp_movie_compatibility(extension: &str) -> FvpMovieCompatibility {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "wmv" | "asf" | "mpg" | "mpeg" => FvpMovieCompatibility::Native,
        "mp4" | "m4v" => FvpMovieCompatibility::PlatformProviderRequired,
        _ => FvpMovieCompatibility::Unsupported,
    }
}

pub fn decode_fvp_movie(
    extension: &str,
    bytes: &[u8],
    max_frames: usize,
    max_decoded_bytes: usize,
    max_audio_samples: usize,
) -> Result<FvpDecodedMovie, FvpMovieDecodeError> {
    let mut decoder = open_fvp_movie_stream(
        extension,
        Arc::from(bytes),
        max_frames,
        max_decoded_bytes,
        max_audio_samples,
    )?;
    let mut movie = FvpDecodedMovie {
        frames: Vec::new(),
        audio: None,
        duration_ms: 0,
    };
    loop {
        match decoder.next_packet()? {
            FvpMoviePacket::Video(frame) => movie.frames.push(frame),
            FvpMoviePacket::Audio(chunk) => {
                let audio = movie.audio.get_or_insert_with(|| FvpMovieAudio {
                    sample_rate: chunk.sample_rate,
                    channels: chunk.channels,
                    samples: Vec::new(),
                });
                audio.samples.extend(chunk.samples);
            }
            FvpMoviePacket::End => break,
        }
    }
    normalize_timeline(&mut movie)?;
    Ok(movie)
}

pub fn open_fvp_movie_stream(
    extension: &str,
    bytes: Arc<[u8]>,
    max_frames: usize,
    max_decoded_bytes: usize,
    max_audio_samples: usize,
) -> Result<FvpMovieStreamDecoder, FvpMovieDecodeError> {
    if bytes.is_empty() || max_frames == 0 || max_decoded_bytes == 0 || max_audio_samples == 0 {
        return Err(FvpMovieDecodeError::Budget);
    }
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    let inner = match extension.as_str() {
        "wmv" | "asf" => {
            let video = AsfWmv2Decoder::open(Cursor::new(Arc::clone(&bytes)))
                .map_err(|_| FvpMovieDecodeError::Decode)?;
            let audio = match AsfWmaDecoder::open(Cursor::new(Arc::clone(&bytes))) {
                Ok(decoder) => Some(decoder),
                Err(wmv_decoder::DecoderError::Unsupported(_)) => None,
                Err(_) => return Err(FvpMovieDecodeError::Decode),
            };
            StreamDecoder::Wmv(Box::new(WmvStreamDecoder {
                video,
                audio,
                pending_video: None,
                pending_audio: None,
                video_eof: false,
                audio_eof: false,
            }))
        }
        "mpg" | "mpeg" => StreamDecoder::Mpeg(Box::new(MpegStreamDecoder {
            pipeline: MpegAvPipeline::new(),
            bytes,
            offset: 0,
            pending: VecDeque::new(),
            flushed: false,
        })),
        _ => return Err(FvpMovieDecodeError::Unsupported),
    };
    Ok(FvpMovieStreamDecoder {
        inner,
        max_frames,
        max_decoded_bytes,
        max_audio_samples,
        frame_count: 0,
        decoded_bytes: 0,
        audio_samples: 0,
        previous_video_pts: None,
        audio_format: None,
        ended: false,
    })
}

impl FvpMovieStreamDecoder {
    pub fn next_packet(&mut self) -> Result<FvpMoviePacket, FvpMovieDecodeError> {
        if self.ended {
            return Ok(FvpMoviePacket::End);
        }
        let packet = match &mut self.inner {
            StreamDecoder::Wmv(decoder) => decoder.next_packet()?,
            StreamDecoder::Mpeg(decoder) => decoder.next_packet()?,
        };
        match &packet {
            FvpMoviePacket::Video(frame) => {
                if frame.width == 0
                    || frame.height == 0
                    || self
                        .previous_video_pts
                        .is_some_and(|previous| frame.pts_ms < previous)
                {
                    return Err(FvpMovieDecodeError::Timeline);
                }
                self.frame_count = self
                    .frame_count
                    .checked_add(1)
                    .filter(|count| *count <= self.max_frames)
                    .ok_or(FvpMovieDecodeError::Budget)?;
                self.decoded_bytes = self
                    .decoded_bytes
                    .checked_add(frame.rgba8.len())
                    .filter(|bytes| *bytes <= self.max_decoded_bytes)
                    .ok_or(FvpMovieDecodeError::Budget)?;
                self.previous_video_pts = Some(frame.pts_ms);
            }
            FvpMoviePacket::Audio(chunk) => {
                if chunk.sample_rate == 0
                    || chunk.channels == 0
                    || chunk.samples.is_empty()
                    || !chunk.samples.len().is_multiple_of(chunk.channels as usize)
                {
                    return Err(FvpMovieDecodeError::Decode);
                }
                if self
                    .audio_format
                    .is_some_and(|format| format != (chunk.sample_rate, chunk.channels))
                {
                    return Err(FvpMovieDecodeError::Decode);
                }
                self.audio_format = Some((chunk.sample_rate, chunk.channels));
                self.audio_samples = self
                    .audio_samples
                    .checked_add(chunk.samples.len())
                    .filter(|samples| *samples <= self.max_audio_samples)
                    .ok_or(FvpMovieDecodeError::Budget)?;
            }
            FvpMoviePacket::End => {
                self.ended = true;
                if self.frame_count == 0 {
                    return Err(FvpMovieDecodeError::Decode);
                }
            }
        }
        Ok(packet)
    }
}

impl WmvStreamDecoder {
    fn next_packet(&mut self) -> Result<FvpMoviePacket, FvpMovieDecodeError> {
        if self.pending_video.is_none() && !self.video_eof {
            self.pending_video = self
                .video
                .next_frame()
                .map_err(|_| FvpMovieDecodeError::Decode)?
                .map(|frame| {
                    Ok(FvpMovieFrame {
                        pts_ms: frame.pts_ms.into(),
                        width: frame.frame.width,
                        height: frame.frame.height,
                        rgba8: yuv420_to_rgba(&frame.frame)?,
                    })
                })
                .transpose()?;
            self.video_eof = self.pending_video.is_none();
        }
        if self.pending_audio.is_none() && !self.audio_eof {
            self.pending_audio = match self.audio.as_mut() {
                Some(audio) => audio
                    .next_frame()
                    .map_err(|_| FvpMovieDecodeError::Decode)?
                    .map(|frame| FvpMovieAudioChunk {
                        pts_ms: frame.pts_ms.into(),
                        sample_rate: audio.sample_rate(),
                        channels: audio.channels(),
                        samples: frame.frame.samples,
                    }),
                None => None,
            };
            self.audio_eof = self.pending_audio.is_none();
        }
        match (&self.pending_video, &self.pending_audio) {
            (Some(video), Some(audio)) if audio.pts_ms < video.pts_ms => Ok(FvpMoviePacket::Audio(
                self.pending_audio
                    .take()
                    .expect("pending audio was checked"),
            )),
            (Some(_), _) => Ok(FvpMoviePacket::Video(
                self.pending_video
                    .take()
                    .expect("pending video was checked"),
            )),
            (None, Some(_)) => Ok(FvpMoviePacket::Audio(
                self.pending_audio
                    .take()
                    .expect("pending audio was checked"),
            )),
            (None, None) => Ok(FvpMoviePacket::End),
        }
    }
}

impl MpegStreamDecoder {
    fn next_packet(&mut self) -> Result<FvpMoviePacket, FvpMovieDecodeError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(match event {
                    MpegAvEvent::Video(frame) => FvpMoviePacket::Video(FvpMovieFrame {
                        pts_ms: frame.pts_ms.max(0) as u64,
                        width: frame.width,
                        height: frame.height,
                        rgba8: frame.rgba,
                    }),
                    MpegAvEvent::Audio(chunk) => FvpMoviePacket::Audio(FvpMovieAudioChunk {
                        pts_ms: chunk.pts_ms.max(0) as u64,
                        sample_rate: chunk.sample_rate,
                        channels: chunk.channels,
                        samples: chunk.samples,
                    }),
                });
            }
            if self.offset < self.bytes.len() {
                let end = self
                    .offset
                    .saturating_add(MPEG_INPUT_CHUNK_BYTES)
                    .min(self.bytes.len());
                self.pipeline
                    .push_with(&self.bytes[self.offset..end], None, |event| {
                        self.pending.push_back(event)
                    })
                    .map_err(|_| FvpMovieDecodeError::Decode)?;
                self.offset = end;
                if self.pending.len() > MAX_PENDING_PACKETS {
                    return Err(FvpMovieDecodeError::Budget);
                }
                continue;
            }
            if !self.flushed {
                self.pipeline
                    .flush_with(|event| self.pending.push_back(event))
                    .map_err(|_| FvpMovieDecodeError::Decode)?;
                self.flushed = true;
                if self.pending.len() > MAX_PENDING_PACKETS {
                    return Err(FvpMovieDecodeError::Budget);
                }
                continue;
            }
            return Ok(FvpMoviePacket::End);
        }
    }
}

fn normalize_timeline(movie: &mut FvpDecodedMovie) -> Result<(), FvpMovieDecodeError> {
    if movie.frames.is_empty() {
        return Err(FvpMovieDecodeError::Decode);
    }
    let mut previous = None;
    for frame in &movie.frames {
        if frame.width == 0 || frame.height == 0 || previous.is_some_and(|pts| frame.pts_ms < pts) {
            return Err(FvpMovieDecodeError::Timeline);
        }
        previous = Some(frame.pts_ms);
    }
    let last = movie
        .frames
        .last()
        .ok_or(FvpMovieDecodeError::Decode)?
        .pts_ms;
    let inferred_step = movie
        .frames
        .windows(2)
        .filter_map(|pair| pair[1].pts_ms.checked_sub(pair[0].pts_ms))
        .find(|value| *value > 0)
        .unwrap_or(34);
    movie.duration_ms = last
        .checked_add(inferred_step)
        .ok_or(FvpMovieDecodeError::Timeline)?;
    Ok(())
}

fn yuv420_to_rgba(frame: &YuvFrame) -> Result<Vec<u8>, FvpMovieDecodeError> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let expected_y = width
        .checked_mul(height)
        .ok_or(FvpMovieDecodeError::Budget)?;
    let chroma_width = width.div_ceil(2);
    let chroma_height = height.div_ceil(2);
    let expected_chroma = chroma_width
        .checked_mul(chroma_height)
        .ok_or(FvpMovieDecodeError::Budget)?;
    if frame.y.len() < expected_y
        || frame.cb.len() < expected_chroma
        || frame.cr.len() < expected_chroma
    {
        return Err(FvpMovieDecodeError::Decode);
    }
    let mut rgba = Vec::with_capacity(
        expected_y
            .checked_mul(4)
            .ok_or(FvpMovieDecodeError::Budget)?,
    );
    for y in 0..height {
        for x in 0..width {
            let luma = f32::from(frame.y[y * width + x]) - 16.0;
            let cb = f32::from(frame.cb[(y / 2) * chroma_width + x / 2]) - 128.0;
            let cr = f32::from(frame.cr[(y / 2) * chroma_width + x / 2]) - 128.0;
            let red = (1.164 * luma + 1.596 * cr).round().clamp(0.0, 255.0) as u8;
            let green = (1.164 * luma - 0.392 * cb - 0.813 * cr)
                .round()
                .clamp(0.0, 255.0) as u8;
            let blue = (1.164 * luma + 2.017 * cb).round().clamp(0.0, 255.0) as u8;
            rgba.extend_from_slice(&[red, green, blue, 255]);
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_probe_distinguishes_native_platform_and_unsupported() {
        assert_eq!(
            fvp_movie_compatibility("WMV"),
            FvpMovieCompatibility::Native
        );
        assert_eq!(
            fvp_movie_compatibility(".mpeg"),
            FvpMovieCompatibility::Native
        );
        assert_eq!(
            fvp_movie_compatibility("mp4"),
            FvpMovieCompatibility::PlatformProviderRequired
        );
        assert_eq!(
            fvp_movie_compatibility("avi"),
            FvpMovieCompatibility::Unsupported
        );
    }

    #[test]
    fn timeline_requires_frames_and_monotonic_pts() {
        let mut empty = FvpDecodedMovie {
            frames: Vec::new(),
            audio: None,
            duration_ms: 0,
        };
        assert!(matches!(
            normalize_timeline(&mut empty),
            Err(FvpMovieDecodeError::Decode)
        ));
        let mut movie = FvpDecodedMovie {
            frames: vec![
                FvpMovieFrame {
                    pts_ms: 20,
                    width: 2,
                    height: 2,
                    rgba8: vec![0; 16],
                },
                FvpMovieFrame {
                    pts_ms: 10,
                    width: 2,
                    height: 2,
                    rgba8: vec![0; 16],
                },
            ],
            audio: None,
            duration_ms: 0,
        };
        assert!(matches!(
            normalize_timeline(&mut movie),
            Err(FvpMovieDecodeError::Timeline)
        ));
    }

    #[test]
    fn decode_rejects_empty_and_unknown_inputs_before_codec_dispatch() {
        assert!(matches!(
            decode_fvp_movie("wmv", &[], 1, 16, 16),
            Err(FvpMovieDecodeError::Budget)
        ));
        assert!(matches!(
            decode_fvp_movie("avi", &[1], 1, 16, 16),
            Err(FvpMovieDecodeError::Unsupported)
        ));
    }
}
