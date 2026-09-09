use std::{io::Cursor, sync::Arc};

use astra_emu_family_api::{FamilyError, FamilyResult};
use wmv_decoder::{AsfWmaDecoder, AsfWmv2Decoder, DecodedFrame, YuvFrame};

fn invalid(code: &'static str, message: &'static str) -> FamilyError {
    FamilyError::invalid(code, message)
}

fn decode_failure() -> FamilyError {
    invalid("ASTRA_EMU_FVP_VIDEO_DECODE", "WMV video decoding failed")
}

/// A WMV2 stream owned by one FVP session. Decoding is deliberately driven by
/// the session's elapsed time so frame presentation remains tied to the
/// family lifecycle; no detached decoder thread can outlive `close`.
pub(crate) struct VideoPlayback {
    decoder: AsfWmv2Decoder<Cursor<Arc<[u8]>>>,
    next: Option<DecodedFrame>,
    current_rgba: Vec<u8>,
    stage_width: u32,
    stage_height: u32,
    base_pts_ms: u32,
    presentation_duration_ns: u64,
    elapsed_ns: u64,
    eof: bool,
    modal_with_audio: bool,
}

impl VideoPlayback {
    pub(crate) fn open(
        bytes: Arc<[u8]>,
        stage_width: u32,
        stage_height: u32,
        modal_with_audio: bool,
    ) -> FamilyResult<Self> {
        if bytes.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_FVP_VIDEO_BYTES",
                "WMV resource is empty",
            ));
        }
        if stage_width == 0 || stage_height == 0 {
            return Err(invalid(
                "ASTRA_EMU_FVP_VIDEO_SIZE",
                "WMV stage dimensions must be non-zero",
            ));
        }

        let mut decoder = AsfWmv2Decoder::open(Cursor::new(bytes)).map_err(|_| decode_failure())?;
        let presentation_duration_ns = decoder
            .presentation_duration_ns()
            .map_err(|_| decode_failure())?;
        if presentation_duration_ns == 0 {
            return Err(invalid(
                "ASTRA_EMU_FVP_VIDEO_DURATION",
                "WMV presentation duration is zero",
            ));
        }
        let first = decoder
            .next_frame()
            .map_err(|_| decode_failure())?
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_EMPTY", "WMV has no video frames"))?;
        let base_pts_ms = first.pts_ms;
        let current_rgba = scale_frame(&first.frame, stage_width, stage_height)?;
        let next = decoder.next_frame().map_err(|_| decode_failure())?;

        Ok(Self {
            decoder,
            next,
            current_rgba,
            stage_width,
            stage_height,
            base_pts_ms,
            presentation_duration_ns,
            elapsed_ns: 0,
            eof: false,
            modal_with_audio,
        })
    }

    pub(crate) fn advance(&mut self, elapsed_ns: u64) -> FamilyResult<bool> {
        self.elapsed_ns = self
            .elapsed_ns
            .checked_add(elapsed_ns)
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_CLOCK", "WMV playback clock overflow"))?;
        let timeline_ns = self.elapsed_ns.min(self.presentation_duration_ns);
        let target_pts_ms = u64::from(self.base_pts_ms)
            .checked_add(timeline_ns / 1_000_000)
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_CLOCK", "WMV PTS overflow"))?;

        while let Some(frame) = self.next.take() {
            if u64::from(frame.pts_ms) > target_pts_ms {
                self.next = Some(frame);
                break;
            }
            self.current_rgba = scale_frame(&frame.frame, self.stage_width, self.stage_height)?;
            self.next = self.decoder.next_frame().map_err(|_| decode_failure())?;
        }
        // ASF play duration includes preroll. `presentation_duration_ns` is
        // the checked container duration after subtracting that preroll, so
        // the final frame remains visible until the media timeline ends even
        // when the decoder has no next frame from which to guess a period.
        self.eof = self.elapsed_ns >= self.presentation_duration_ns;

        Ok(self.eof)
    }

    pub(crate) fn frame(&self) -> &[u8] {
        &self.current_rgba
    }

    pub(crate) fn modal_with_audio(&self) -> bool {
        self.modal_with_audio
    }
}

/// A WMA stream owned by the FVP audio worker. The decoder is advanced only as
/// output samples are consumed, keeping decoded PCM bounded to the current
/// decoder frame rather than expanding the whole movie into memory.
pub(crate) struct VideoAudioPlayback {
    decoder: AsfWmaDecoder<Cursor<Arc<[u8]>>>,
    sample_rate: u32,
    channels: u16,
    pending: Vec<f32>,
    pending_start_frame: u64,
    output_frames: u64,
    source_eof: bool,
    finished: bool,
}

impl VideoAudioPlayback {
    /// Open the optional WMA track. ASF files without an audio stream return
    /// `None`; a declared but unsupported or empty track is an error.
    pub(crate) fn open(bytes: Arc<[u8]>) -> FamilyResult<Option<Self>> {
        let mut inspect = Cursor::new(Arc::clone(&bytes));
        let asf = wmv_decoder::asf::AsfFile::open(&mut inspect).map_err(|_| audio_failure())?;
        if asf.audio_streams.is_empty() {
            return Ok(None);
        }

        let mut decoder = AsfWmaDecoder::open(Cursor::new(bytes)).map_err(|_| audio_failure())?;
        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        validate_audio_format(sample_rate, channels)?;
        let first = decoder
            .next_frame()
            .map_err(|_| audio_failure())?
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_EMPTY", "WMV audio is empty"))?;
        validate_audio_frame(sample_rate, channels, &first.frame)?;
        if first.frame.samples.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_FVP_VIDEO_AUDIO_EMPTY",
                "WMV audio is empty",
            ));
        }

        Ok(Some(Self {
            decoder,
            sample_rate,
            channels,
            pending: first.frame.samples,
            pending_start_frame: 0,
            output_frames: 0,
            source_eof: false,
            finished: false,
        }))
    }

    pub(crate) fn mix_into(&mut self, output_rate: u32, output: &mut [i16]) -> FamilyResult<bool> {
        if output_rate == 0 || !output.len().is_multiple_of(2) {
            return Err(invalid(
                "ASTRA_EMU_FVP_VIDEO_AUDIO_FORMAT",
                "audio output format is invalid",
            ));
        }
        if self.finished {
            return Ok(false);
        }

        let output_frames = output.len() / 2;
        let mut rendered_frames = 0_u64;
        for output_frame in 0..output_frames {
            let frame_index = self
                .output_frames
                .checked_add(output_frame as u64)
                .ok_or_else(|| {
                    invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_CLOCK", "audio clock overflow")
                })?;
            let source_frame = frame_index
                .checked_mul(u64::from(self.sample_rate))
                .ok_or_else(|| {
                    invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_CLOCK", "audio clock overflow")
                })?
                / u64::from(output_rate);
            if !self.ensure_source_frame(source_frame)? {
                self.finished = true;
                break;
            }

            let local_frame =
                usize::try_from(source_frame - self.pending_start_frame).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                        "audio frame index overflows",
                    )
                })?;
            let source_offset = local_frame
                .checked_mul(usize::from(self.channels))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                        "audio sample index overflows",
                    )
                })?;
            let left = self.pending[source_offset];
            let right = if self.channels == 1 {
                left
            } else {
                self.pending[source_offset + 1]
            };
            let destination = output_frame * 2;
            output[destination] = add_sample(output[destination], left);
            output[destination + 1] = add_sample(output[destination + 1], right);
            rendered_frames = rendered_frames.checked_add(1).ok_or_else(|| {
                invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_CLOCK", "audio clock overflow")
            })?;
        }
        self.output_frames = self
            .output_frames
            .checked_add(rendered_frames)
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_CLOCK", "audio clock overflow"))?;
        self.discard_consumed_samples(output_rate)?;
        Ok(rendered_frames != 0)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    fn ensure_source_frame(&mut self, source_frame: u64) -> FamilyResult<bool> {
        loop {
            let pending_frames = self.pending.len() / usize::from(self.channels);
            let pending_end = self
                .pending_start_frame
                .checked_add(pending_frames as u64)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                        "audio frame count overflows",
                    )
                })?;
            if source_frame < pending_end {
                return Ok(true);
            }
            if self.source_eof {
                return Ok(false);
            }
            let Some(frame) = self.decoder.next_frame().map_err(|_| audio_failure())? else {
                self.source_eof = true;
                return Ok(false);
            };
            validate_audio_frame(self.sample_rate, self.channels, &frame.frame)?;
            self.pending.extend_from_slice(&frame.frame.samples);
        }
    }

    fn discard_consumed_samples(&mut self, output_rate: u32) -> FamilyResult<()> {
        let next_source_frame = self
            .output_frames
            .checked_mul(u64::from(self.sample_rate))
            .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_AUDIO_CLOCK", "audio clock overflow"))?
            / u64::from(output_rate);
        let pending_end = self
            .pending_start_frame
            .checked_add((self.pending.len() / usize::from(self.channels)) as u64)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                    "audio frame count overflows",
                )
            })?;
        let discard = next_source_frame
            .saturating_sub(self.pending_start_frame)
            .min(pending_end.saturating_sub(self.pending_start_frame));
        if discard != 0 {
            let samples = usize::try_from(discard)
                .ok()
                .and_then(|frames| frames.checked_mul(usize::from(self.channels)))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                        "audio sample count overflows",
                    )
                })?;
            self.pending.drain(..samples);
            self.pending_start_frame =
                self.pending_start_frame
                    .checked_add(discard)
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_FVP_VIDEO_AUDIO_SIZE",
                            "audio frame count overflows",
                        )
                    })?;
        }
        Ok(())
    }
}

fn audio_failure() -> FamilyError {
    invalid(
        "ASTRA_EMU_FVP_VIDEO_AUDIO_DECODE",
        "WMV audio decoding failed",
    )
}

fn validate_audio_format(sample_rate: u32, channels: u16) -> FamilyResult<()> {
    if sample_rate == 0 || channels == 0 || channels > 2 {
        return Err(invalid(
            "ASTRA_EMU_FVP_VIDEO_AUDIO_FORMAT",
            "WMV audio channel format is unsupported",
        ));
    }
    Ok(())
}

fn validate_audio_frame(
    sample_rate: u32,
    channels: u16,
    frame: &wmv_decoder::wma::PcmFrameF32,
) -> FamilyResult<()> {
    validate_audio_format(frame.sample_rate, frame.channels)?;
    if frame.sample_rate != sample_rate || frame.channels != channels {
        return Err(invalid(
            "ASTRA_EMU_FVP_VIDEO_AUDIO_FORMAT",
            "WMV audio format changes during playback",
        ));
    }
    if !frame.samples.len().is_multiple_of(usize::from(channels)) {
        return Err(invalid(
            "ASTRA_EMU_FVP_VIDEO_AUDIO_ALIGNMENT",
            "WMV audio samples are not channel aligned",
        ));
    }
    Ok(())
}

fn add_sample(destination: i16, source: f32) -> i16 {
    let source = if source.is_finite() {
        source.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let source = (source * 32_767.0).round() as i32;
    (i32::from(destination) + source).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn scale_frame(src: &YuvFrame, dst_width: u32, dst_height: u32) -> FamilyResult<Vec<u8>> {
    validate_yuv_frame(src)?;
    let source_width = src.width;
    let source_height = src.height;
    let uv_width = source_width.div_ceil(2);
    let uv_height = source_height.div_ceil(2);
    let pixels = usize::try_from(dst_width)
        .ok()
        .and_then(|width| {
            usize::try_from(dst_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_SIZE", "WMV frame dimensions overflow"))?;
    let output_len = pixels
        .checked_mul(4)
        .ok_or_else(|| invalid("ASTRA_EMU_FVP_VIDEO_SIZE", "WMV frame byte size overflows"))?;
    let mut output = vec![0_u8; output_len];
    for y in 0..dst_height {
        let source_y = ((u64::from(y) * u64::from(source_height)) / u64::from(dst_height))
            .min(u64::from(source_height - 1)) as u32;
        let source_uv_y = (source_y / 2).min(uv_height - 1);
        for x in 0..dst_width {
            let source_x = ((u64::from(x) * u64::from(source_width)) / u64::from(dst_width))
                .min(u64::from(source_width - 1)) as u32;
            let source_uv_x = (source_x / 2).min(uv_width - 1);
            let y_sample = src.y[(source_y * source_width + source_x) as usize] as i32;
            let u_sample = src.cb[(source_uv_y * uv_width + source_uv_x) as usize] as i32;
            let v_sample = src.cr[(source_uv_y * uv_width + source_uv_x) as usize] as i32;
            let c = y_sample - 16;
            let d = u_sample - 128;
            let e = v_sample - 128;
            let offset = ((y * dst_width + x) as usize) * 4;
            output[offset] = clamp((298 * c + 409 * e + 128) >> 8);
            output[offset + 1] = clamp((298 * c - 100 * d - 208 * e + 128) >> 8);
            output[offset + 2] = clamp((298 * c + 516 * d + 128) >> 8);
            output[offset + 3] = 255;
        }
    }
    Ok(output)
}

fn validate_yuv_frame(src: &YuvFrame) -> FamilyResult<()> {
    if src.width == 0 || src.height == 0 {
        return Err(invalid(
            "ASTRA_EMU_FVP_VIDEO_FRAME_DIMENSIONS",
            "WMV frame dimensions must be non-zero",
        ));
    }
    let y_len = usize::try_from(src.width)
        .ok()
        .and_then(|width| {
            usize::try_from(src.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_FVP_VIDEO_FRAME_DIMENSIONS",
                "WMV frame dimensions overflow",
            )
        })?;
    let uv_width = src.width.checked_add(1).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_FVP_VIDEO_FRAME_DIMENSIONS",
            "WMV chroma dimensions overflow",
        )
    })? / 2;
    let uv_height = src.height.checked_add(1).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_FVP_VIDEO_FRAME_DIMENSIONS",
            "WMV chroma dimensions overflow",
        )
    })? / 2;
    let uv_len = usize::try_from(uv_width)
        .ok()
        .and_then(|width| {
            usize::try_from(uv_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_FVP_VIDEO_FRAME_DIMENSIONS",
                "WMV chroma dimensions overflow",
            )
        })?;
    if src.y.len() != y_len || src.cb.len() != uv_len || src.cr.len() != uv_len {
        return Err(invalid(
            "ASTRA_EMU_FVP_VIDEO_FRAME_PLANES",
            "WMV YUV planes do not match the declared dimensions",
        ));
    }
    Ok(())
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_frame_converts_and_scales_yuv420() {
        let source = YuvFrame {
            width: 2,
            height: 2,
            y: vec![16, 235, 16, 235],
            cb: vec![128],
            cr: vec![128],
        };
        let output = scale_frame(&source, 4, 4).expect("bounded YUV frame scales");
        assert_eq!(output.len(), 4 * 4 * 4);
        assert_eq!(&output[0..4], &[0, 0, 0, 255]);
        assert_eq!(&output[(2 * 4)..(2 * 4 + 4)], &[255, 255, 255, 255]);
        assert_eq!(&output[(2 * 4 * 4)..(2 * 4 * 4 + 4)], &[0, 0, 0, 255]);
    }

    #[test]
    fn malformed_wmv_fails_before_playback_state_is_created() {
        let error = VideoPlayback::open(Arc::from(vec![0_u8; 4]), 2, 2, false)
            .err()
            .expect("malformed WMV must fail");
        assert_eq!(error.code(), "ASTRA_EMU_FVP_VIDEO_DECODE");
    }

    #[test]
    fn scale_frame_rejects_invalid_yuv_planes() {
        let source = YuvFrame {
            width: 3,
            height: 3,
            y: vec![16; 9],
            cb: vec![128; 3],
            cr: vec![128; 4],
        };
        let error =
            scale_frame(&source, 3, 3).expect_err("truncated odd-sized chroma plane must fail");
        assert_eq!(error.code(), "ASTRA_EMU_FVP_VIDEO_FRAME_PLANES");
    }

    #[test]
    fn scale_frame_accepts_odd_yuv_dimensions() {
        let source = YuvFrame {
            width: 3,
            height: 3,
            y: vec![16; 9],
            cb: vec![128; 4],
            cr: vec![128; 4],
        };
        let output = scale_frame(&source, 3, 3).expect("odd-sized YUV frame scales");
        assert_eq!(output.len(), 3 * 3 * 4);
    }
}
