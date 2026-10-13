use super::*;

pub(in super::super) fn startup() -> Result<WmfSession, MediaError> {
    WmfSession::new().map_err(|err| {
        blocking(
            "ASTRA_WMF_PROBE",
            format!("Media Foundation startup failed: {err}"),
        )
    })
}

pub(in super::super) fn decode_audio(
    provider_id: String,
    request: &DecodeRequest,
) -> Result<DecodeResult, MediaError> {
    let _session = startup()?;
    let output = decode_audio_inner(&request.bytes).map_err(decode_error)?;
    Ok(DecodeResult {
        provider_id,
        kind: DecodeKind::Audio,
        codec: request.codec.clone(),
        output: DecodeOutput::AudioPcmI16 {
            samples: output.samples,
            sample_rate: output.sample_rate,
            channels: u16::try_from(output.channels)
                .map_err(|_| blocking("ASTRA_WMF_AUDIO_CHANNELS", "channel count exceeds u16"))?,
        },
        diagnostics: output.diagnostics,
    })
}

pub(in super::super) fn decode_video(
    provider_id: String,
    request: &DecodeRequest,
) -> Result<DecodeResult, MediaError> {
    let _session = startup()?;
    let output = decode_video_inner(&request.bytes).map_err(decode_error)?;
    Ok(DecodeResult {
        provider_id,
        kind: DecodeKind::Video,
        codec: request.codec.clone(),
        output: DecodeOutput::CpuBuffer {
            bytes: output.bgra.into(),
            format: format!("bgra8:first_frame:{}x{}", output.width, output.height),
        },
        diagnostics: Vec::new(),
    })
}

pub(in super::super) fn decode_video_stream(
    bytes: &[u8],
    max_frames: u64,
    max_bytes: u64,
) -> Result<crate::DecodedVideoStream, MediaError> {
    let _session = startup()?;
    let stream = decode_video_stream_inner(bytes, max_frames, max_bytes).map_err(decode_error)?;
    stream.validate(max_frames, max_bytes)?;
    Ok(stream)
}

pub(in super::super) fn blocking(code: &'static str, message: impl Into<String>) -> MediaError {
    MediaError::Diagnostics(vec![Diagnostic::blocking(code, message.into())])
}

pub(in super::super) fn decode_error(err: WindowsError) -> MediaError {
    blocking(
        "ASTRA_WMF_DECODE",
        format!("Media Foundation decode failed: {err}"),
    )
}

pub(super) struct AudioOutput {
    pub(super) samples: Vec<i16>,
    pub(super) sample_rate: u32,
    pub(super) channels: u32,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) struct VideoOutput {
    pub(super) bgra: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(in super::super) struct IncrementalVideoDecoder {
    pub(super) reader: windows::Win32::Media::MediaFoundation::IMFSourceReader,
    pub(super) stream_index: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) expected_frame_bytes: usize,
    pub(super) frame_duration_us: u64,
    pub(in super::super) duration_us: u64,
    pub(super) max_frames: u64,
    pub(super) max_bytes: u64,
    pub(super) emitted_frames: u64,
    pub(super) emitted_bytes: u64,
    pub(super) previous_pts_us: Option<u64>,
    pub(super) eof: bool,
    // Must be dropped after every Media Foundation interface above.
    pub(super) _session: WmfSession,
}

pub(in super::super) struct IncrementalAudioDecoder {
    pub(super) reader: windows::Win32::Media::MediaFoundation::IMFSourceReader,
    pub(super) stream_index: u32,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) max_samples: u64,
    pub(super) emitted_samples: u64,
    pub(super) previous_pts_us: Option<u64>,
    pub(super) eof: bool,
    // Must be dropped after every Media Foundation interface above.
    pub(super) _session: WmfSession,
}

impl IncrementalAudioDecoder {
    pub(in super::super) fn open(bytes: &[u8], max_samples: u64) -> windows::core::Result<Self> {
        unsafe {
            if max_samples == 0 {
                return Err(wmf_error("audio stream decode budget is empty"));
            }
            let session = WmfSession::new()?;
            let setup = audio_stream_setup(bytes)?;
            let (sample_rate, channels) = audio_format_from_media_type(&setup.media_type)?;
            Ok(Self {
                reader: setup.reader,
                stream_index: setup.stream_index,
                sample_rate,
                channels,
                max_samples,
                emitted_samples: 0,
                previous_pts_us: None,
                eof: false,
                _session: session,
            })
        }
    }

    pub(in super::super) fn next_chunk(
        &mut self,
    ) -> windows::core::Result<Option<WindowsDecodedAudioChunk>> {
        if self.eof {
            return Ok(None);
        }
        unsafe {
            loop {
                let mut flags = 0;
                let mut timestamp_100ns = 0_i64;
                let mut sample = None;
                self.reader.ReadSample(
                    self.stream_index,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp_100ns),
                    Some(&mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    self.eof = true;
                    return Ok(None);
                }
                let Some(sample) = sample else {
                    continue;
                };
                let samples = sample_i16(&sample)?;
                if samples.is_empty()
                    || samples.len().saturating_mul(std::mem::size_of::<i16>())
                        > MAX_DECODED_AUDIO_BYTES
                    || !samples.len().is_multiple_of(usize::from(self.channels))
                    || timestamp_100ns < 0
                {
                    return Err(wmf_error(
                        "audio decode produced an invalid PCM chunk or timestamp",
                    ));
                }
                let chunk_samples = u64::try_from(samples.len())
                    .map_err(|_| wmf_error("decoded audio sample count overflowed"))?;
                let emitted_samples = self
                    .emitted_samples
                    .checked_add(chunk_samples)
                    .filter(|value| *value <= self.max_samples)
                    .ok_or_else(|| wmf_error("decoded audio exceeds the sample budget"))?;
                let pts_us = timestamp_100ns as u64 / 10;
                if self
                    .previous_pts_us
                    .is_some_and(|previous| pts_us < previous)
                {
                    return Err(wmf_error("decoded audio timestamps are not ordered"));
                }
                self.emitted_samples = emitted_samples;
                self.previous_pts_us = Some(pts_us);
                return Ok(Some(WindowsDecodedAudioChunk {
                    pts_us,
                    sample_rate: self.sample_rate,
                    channels: self.channels,
                    samples,
                }));
            }
        }
    }
}

impl IncrementalVideoDecoder {
    pub(in super::super) fn open(
        bytes: &[u8],
        max_frames: u64,
        max_bytes: u64,
    ) -> windows::core::Result<Self> {
        unsafe {
            if max_frames == 0 || max_bytes == 0 {
                return Err(wmf_error("video stream decode budget is empty"));
            }
            let session = WmfSession::new()?;
            let reader = source_reader_from_bytes(bytes)?;
            let duration = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)?;
            let duration_us = u64::try_from(&duration)? / 10;
            if duration_us == 0 {
                return Err(wmf_error("video decode reported an empty duration"));
            }
            let stream_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            let media_type = media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(stream_index, None, &media_type)?;
            let current_type = reader.GetCurrentMediaType(stream_index)?;
            let Some((width, height)) = attribute_frame_size(&current_type) else {
                return Err(wmf_error("video decode did not report a frame size"));
            };
            let frame_duration_us = attribute_frame_duration_us(&current_type);
            let expected_frame_bytes = usize::try_from(width)
                .ok()
                .and_then(|width| {
                    usize::try_from(height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| wmf_error("video frame dimensions overflowed"))?;
            if expected_frame_bytes == 0 || expected_frame_bytes > MAX_DECODED_VIDEO_FRAME_BYTES {
                return Err(wmf_error(
                    "video frame exceeds the bounded CPU buffer limit",
                ));
            }
            Ok(Self {
                reader,
                stream_index,
                width,
                height,
                expected_frame_bytes,
                frame_duration_us,
                duration_us,
                max_frames,
                max_bytes,
                emitted_frames: 0,
                emitted_bytes: 0,
                previous_pts_us: None,
                eof: false,
                _session: session,
            })
        }
    }

    pub(in super::super) fn next_frame(
        &mut self,
    ) -> windows::core::Result<Option<crate::DecodedVideoFrame>> {
        if self.eof {
            return Ok(None);
        }
        unsafe {
            loop {
                let mut flags = 0;
                let mut timestamp_100ns = 0_i64;
                let mut sample = None;
                self.reader.ReadSample(
                    self.stream_index,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp_100ns),
                    Some(&mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    self.eof = true;
                    return Ok(None);
                }
                let Some(sample) = sample else {
                    continue;
                };
                let frame_bytes = sample_bytes(&sample)?;
                if frame_bytes.len() < self.expected_frame_bytes || timestamp_100ns < 0 {
                    return Err(wmf_error(
                        "video decode produced a partial frame or invalid timestamp",
                    ));
                }
                if self.emitted_frames >= self.max_frames {
                    return Err(wmf_error("decoded video exceeds the frame budget"));
                }
                let next_bytes = self
                    .emitted_bytes
                    .checked_add(self.expected_frame_bytes as u64)
                    .filter(|value| *value <= self.max_bytes)
                    .ok_or_else(|| wmf_error("decoded video exceeds the byte budget"))?;
                let pts_us = timestamp_100ns as u64 / 10;
                if pts_us >= self.duration_us
                    || self
                        .previous_pts_us
                        .is_some_and(|previous| pts_us < previous)
                {
                    return Err(wmf_error("decoded video timestamps are not ordered"));
                }
                self.emitted_frames += 1;
                self.emitted_bytes = next_bytes;
                self.previous_pts_us = Some(pts_us);
                // `sample_bytes` already owns the one CPU readback from the
                // Media Foundation buffer.  Truncate that allocation in
                // place instead of slicing into a second Vec; the stream
                // path must not copy a complete frame again before it
                // reaches PlatformHost.
                let mut bgra8 = frame_bytes;
                bgra8.truncate(self.expected_frame_bytes);
                return Ok(Some(crate::DecodedVideoFrame {
                    sequence: self.emitted_frames,
                    pts_us,
                    duration_us: self.frame_duration_us.min(self.duration_us - pts_us),
                    width: self.width,
                    height: self.height,
                    bgra8: bgra8.into(),
                }));
            }
        }
    }
}

pub(in super::super) struct WmfSession {
    pub(super) com_initialized: bool,
}

impl WmfSession {
    pub(super) fn new() -> windows::core::Result<Self> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let com_initialized = match hr {
                S_OK | S_FALSE => true,
                RPC_E_CHANGED_MODE => false,
                other => {
                    other.ok()?;
                    false
                }
            };
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            Ok(Self { com_initialized })
        }
    }
}

impl Drop for WmfSession {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            if self.com_initialized {
                CoUninitialize();
            }
        }
    }
}

pub(super) fn decode_audio_inner(bytes: &[u8]) -> windows::core::Result<AudioOutput> {
    unsafe {
        let setup = audio_stream_setup(bytes)?;
        let (sample_rate, channels) = audio_format_from_media_type(&setup.media_type)?;
        let mut samples = Vec::new();

        loop {
            let mut flags = 0;
            let mut sample = None;
            setup.reader.ReadSample(
                setup.stream_index,
                0,
                None,
                Some(&mut flags),
                None,
                Some(&mut sample),
            )?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else {
                continue;
            };
            let chunk = sample_i16(&sample)?;
            if chunk.is_empty() {
                continue;
            }
            let max_samples = MAX_DECODED_AUDIO_BYTES / std::mem::size_of::<i16>();
            if chunk.len() > max_samples.saturating_sub(samples.len()) {
                return Err(wmf_error(
                    "decoded audio exceeds the bounded CPU buffer budget",
                ));
            }
            samples.extend(chunk);
        }

        if samples.is_empty() {
            return Err(wmf_error("audio decode produced no PCM samples"));
        }
        Ok(AudioOutput {
            samples,
            sample_rate,
            channels: u32::from(channels),
            diagnostics: Vec::new(),
        })
    }
}
