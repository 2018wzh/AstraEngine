use super::*;

pub(super) fn decode_video_inner(bytes: &[u8]) -> windows::core::Result<VideoOutput> {
    unsafe {
        let reader = source_reader_from_bytes(bytes)?;
        let stream_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let media_type = media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(stream_index, None, &media_type)?;
        let current_type = reader.GetCurrentMediaType(stream_index)?;
        let Some((width, height)) = attribute_frame_size(&current_type) else {
            return Err(wmf_error("video decode did not report a frame size"));
        };
        let expected_frame_bytes = width as usize * height as usize * 4;
        if expected_frame_bytes == 0 {
            return Err(wmf_error("video decode reported an empty frame size"));
        }
        if expected_frame_bytes > MAX_DECODED_VIDEO_FRAME_BYTES {
            return Err(wmf_error(
                "video frame exceeds the bounded CPU buffer limit",
            ));
        }

        loop {
            let mut flags = 0;
            let mut sample = None;
            reader.ReadSample(
                stream_index,
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
            let bytes = sample_bytes(&sample)?;
            if !bytes.is_empty() {
                if bytes.len() < expected_frame_bytes {
                    return Err(wmf_error("video decode produced a partial BGRA frame"));
                }
                let mut bgra = bytes;
                bgra.truncate(expected_frame_bytes);
                return Ok(VideoOutput {
                    bgra,
                    width,
                    height,
                });
            }
        }
        Err(wmf_error("video decode produced no BGRA frame"))
    }
}

pub(super) fn decode_video_stream_inner(
    bytes: &[u8],
    max_frames: u64,
    max_bytes: u64,
) -> windows::core::Result<crate::DecodedVideoStream> {
    unsafe {
        if max_frames == 0 || max_bytes == 0 {
            return Err(wmf_error("video stream decode budget is empty"));
        }
        let reader = source_reader_from_bytes(bytes)?;
        let stream_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let media_type = media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(stream_index, None, &media_type)?;
        let current_type = reader.GetCurrentMediaType(stream_index)?;
        let Some((width, height)) = attribute_frame_size(&current_type) else {
            return Err(wmf_error("video decode did not report a frame size"));
        };
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
        let mut raw = Vec::new();
        let mut total_bytes = 0_u64;
        loop {
            let mut flags = 0;
            let mut timestamp_100ns = 0_i64;
            let mut sample = None;
            reader.ReadSample(
                stream_index,
                0,
                None,
                Some(&mut flags),
                Some(&mut timestamp_100ns),
                Some(&mut sample),
            )?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else {
                continue;
            };
            let frame_bytes = sample_bytes(&sample)?;
            if frame_bytes.len() < expected_frame_bytes || timestamp_100ns < 0 {
                return Err(wmf_error(
                    "video decode produced a partial frame or invalid timestamp",
                ));
            }
            if raw.len() as u64 >= max_frames {
                return Err(wmf_error("decoded video exceeds the frame budget"));
            }
            total_bytes = total_bytes
                .checked_add(expected_frame_bytes as u64)
                .filter(|value| *value <= max_bytes)
                .ok_or_else(|| wmf_error("decoded video exceeds the byte budget"))?;
            let mut frame_bytes = frame_bytes;
            frame_bytes.truncate(expected_frame_bytes);
            raw.push((timestamp_100ns as u64 / 10, frame_bytes));
        }
        if raw.is_empty() {
            return Err(wmf_error("video decode produced no frames"));
        }
        let fallback_duration_us = raw
            .windows(2)
            .filter_map(|pair| pair[1].0.checked_sub(pair[0].0))
            .find(|duration| *duration > 0)
            .unwrap_or(33_334);
        let duration_us = raw
            .last()
            .and_then(|last| last.0.checked_add(fallback_duration_us))
            .ok_or_else(|| wmf_error("video duration overflowed"))?;
        let mut decoded_frames = Vec::with_capacity(raw.len());
        for index in 0..raw.len() {
            let pts_us = raw[index].0;
            let next_pts = raw.get(index + 1).map_or(duration_us, |frame| frame.0);
            let duration = next_pts
                .checked_sub(pts_us)
                .filter(|value| *value > 0)
                .unwrap_or(fallback_duration_us);
            let bgra8 = std::mem::take(&mut raw[index].1);
            decoded_frames.push(crate::DecodedVideoFrame {
                sequence: index as u64 + 1,
                pts_us,
                duration_us: duration.min(duration_us - pts_us),
                width,
                height,
                bgra8: bgra8.into(),
            });
        }
        Ok(crate::DecodedVideoStream {
            schema: crate::DECODED_VIDEO_STREAM_SCHEMA.into(),
            duration_us,
            frames: decoded_frames,
        })
    }
}

pub(super) struct AudioStreamSetup {
    pub(super) reader: IMFSourceReader,
    pub(super) stream_index: u32,
    pub(super) media_type: IMFMediaType,
}

/// Shared Source Reader setup for both audio decode paths: selects the
/// first audio stream and negotiates PCM output.
pub(super) unsafe fn audio_stream_setup(bytes: &[u8]) -> windows::core::Result<AudioStreamSetup> {
    let reader = source_reader_from_bytes(bytes)?;
    let stream_index = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
    let requested = media_type(&MFMediaType_Audio, &MFAudioFormat_PCM)?;
    reader.SetCurrentMediaType(stream_index, None, &requested)?;
    let media_type = reader.GetCurrentMediaType(stream_index)?;
    Ok(AudioStreamSetup {
        reader,
        stream_index,
        media_type,
    })
}

/// Reads sample rate and channel count from the negotiated PCM media
/// type, applying the shared PCM format contract.
pub(super) unsafe fn audio_format_from_media_type(
    media_type: &IMFMediaType,
) -> windows::core::Result<(u32, u16)> {
    let sample_rate = attribute_u32(media_type, &MF_MT_AUDIO_SAMPLES_PER_SECOND)
        .filter(|value| is_supported_pcm_rate(*value))
        .ok_or_else(|| wmf_error("audio decode reported an invalid sample rate"))?;
    let channels = attribute_u32(media_type, &MF_MT_AUDIO_NUM_CHANNELS)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| is_supported_pcm_channel_count(*value))
        .ok_or_else(|| wmf_error("audio decode reported an invalid channel count"))?;
    Ok((sample_rate, channels))
}

pub(super) unsafe fn source_reader_from_bytes(
    bytes: &[u8],
) -> windows::core::Result<windows::Win32::Media::MediaFoundation::IMFSourceReader> {
    if bytes.is_empty() {
        return Err(wmf_error("decode input is empty"));
    }
    let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
    let locked = GlobalLock(hglobal);
    if locked.is_null() {
        return Err(WindowsError::from_thread());
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
    let _ = GlobalUnlock(hglobal);
    let stream = CreateStreamOnHGlobal(hglobal, true)?;
    let byte_stream = MFCreateMFByteStreamOnStreamEx(&stream)?;
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 2)?;
    let attributes = attributes.ok_or_else(|| wmf_error("source reader attributes unavailable"))?;
    attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
    attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    tracing::debug!(
        event = "media.decode.wmf.hardware_transform.requested",
        hardware_transforms = true,
        output_boundary = "cpu_bgra8_or_pcm_s16le",
        "Media Foundation source reader requested hardware transforms"
    );
    MFCreateSourceReaderFromByteStream(&byte_stream, Some(&attributes))
}

pub(super) unsafe fn media_type(
    major_type: &windows::core::GUID,
    subtype: &windows::core::GUID,
) -> windows::core::Result<IMFMediaType> {
    let media_type = MFCreateMediaType()?;
    let attrs: IMFAttributes = media_type.cast()?;
    attrs.SetGUID(&MF_MT_MAJOR_TYPE, major_type)?;
    attrs.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    Ok(media_type)
}

pub(super) unsafe fn attribute_u32(
    media_type: &IMFMediaType,
    key: &windows::core::GUID,
) -> Option<u32> {
    let attrs: IMFAttributes = media_type.cast().ok()?;
    attrs.GetUINT32(key).ok()
}

pub(super) unsafe fn attribute_frame_size(media_type: &IMFMediaType) -> Option<(u32, u32)> {
    let attrs: IMFAttributes = media_type.cast().ok()?;
    let packed = attrs.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
    let width = (packed >> 32) as u32;
    let height = (packed & 0xffff_ffff) as u32;
    (width > 0 && height > 0).then_some((width, height))
}

pub(super) unsafe fn attribute_frame_duration_us(media_type: &IMFMediaType) -> u64 {
    let attrs: IMFAttributes = match media_type.cast() {
        Ok(attrs) => attrs,
        Err(_) => return 33_333,
    };
    let packed = match attrs.GetUINT64(&MF_MT_FRAME_RATE) {
        Ok(packed) => packed,
        Err(_) => return 33_333,
    };
    let numerator = packed >> 32;
    let denominator = packed & 0xffff_ffff;
    if numerator == 0 || denominator == 0 {
        return 33_333;
    }
    (1_000_000_u64
        .saturating_mul(denominator)
        .saturating_add(numerator / 2)
        / numerator)
        .max(1)
}

pub(super) fn wmf_error(message: &'static str) -> WindowsError {
    WindowsError::new(HRESULT(0x80004005_u32 as i32), message)
}

pub(super) unsafe fn sample_bytes(sample: &IMFSample) -> windows::core::Result<Vec<u8>> {
    let buffer = sample.ConvertToContiguousBuffer()?;
    let len = buffer.GetCurrentLength()? as usize;
    let mut data = ptr::null_mut();
    let mut current_len = 0;
    buffer.Lock(&mut data, None, Some(&mut current_len))?;
    let copy_len = (current_len as usize).min(len);
    let bytes = if copy_len == 0 || data.is_null() {
        Vec::new()
    } else {
        slice::from_raw_parts(data, copy_len).to_vec()
    };
    buffer.Unlock()?;
    Ok(bytes)
}

pub(super) unsafe fn sample_i16(sample: &IMFSample) -> windows::core::Result<Vec<i16>> {
    let buffer = sample.ConvertToContiguousBuffer()?;
    let len = buffer.GetCurrentLength()? as usize;
    let mut data = ptr::null_mut();
    let mut current_len = 0;
    buffer.Lock(&mut data, None, Some(&mut current_len))?;
    let copy_len = (current_len as usize).min(len);
    if !copy_len.is_multiple_of(std::mem::size_of::<i16>()) {
        buffer.Unlock()?;
        return Err(wmf_error("audio sample ends inside an i16 value"));
    }
    let sample_count = copy_len / std::mem::size_of::<i16>();
    let mut samples = Vec::<i16>::with_capacity(sample_count);
    if sample_count != 0 && !data.is_null() {
        ptr::copy_nonoverlapping(
            data.cast::<u8>(),
            samples.as_mut_ptr().cast::<u8>(),
            copy_len,
        );
        samples.set_len(sample_count);
    }
    buffer.Unlock()?;
    Ok(samples)
}
