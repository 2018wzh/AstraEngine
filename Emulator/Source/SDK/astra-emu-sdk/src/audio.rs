use crate::CoreError;
use kira::{
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
    Frame,
};
use std::{
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
};
use symphonia::core::{
    audio::{
        conv::{FromSample, IntoSample},
        sample::Sample,
        Audio, AudioBuffer, GenericAudioBufferRef,
    },
    codecs::CodecParameters,
    formats::TrackType,
    io::MediaSourceStream,
};

/// Decode bounded mono/stereo audio into Kira's native floating-point frames.
/// Cancellation is checked before probing and between decoded packets.
pub fn decode_audio(
    bytes: Vec<u8>,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(CoreError::invalid(
            "ASTRA_EMU_AUDIO_CANCELLED",
            "audio decode cancelled",
        ));
    }
    let fail = || {
        CoreError::invalid(
            "ASTRA_EMU_AUDIO_DECODE",
            "audio could not be fully decoded within its budget",
        )
    };
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Default::default(),
            source,
            Default::default(),
            Default::default(),
        )
        .map_err(|_| fail())?;
    let track = reader.default_track(TrackType::Audio).ok_or_else(fail)?;
    let id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
        return Err(fail());
    };
    let rate = params.sample_rate.ok_or_else(fail)?;
    if !(8000..=192000).contains(&rate) {
        return Err(fail());
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())
        .map_err(|_| fail())?;
    let mut frames = Vec::new();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(CoreError::invalid(
                "ASTRA_EMU_AUDIO_CANCELLED",
                "audio decode cancelled",
            ));
        }
        let Some(packet) = reader.next_packet().map_err(|_| fail())? else {
            break;
        };
        if packet.track_id != id {
            continue;
        }
        let buffer = decoder.decode(&packet).map_err(|_| fail())?;
        if buffer.spec().rate() != rate {
            return Err(fail());
        }
        macro_rules! append {
            ($buffer:expr) => {
                append_frames($buffer, &mut frames, max_frames)?
            };
        }
        match &buffer {
            GenericAudioBufferRef::U8(b) => append!(b),
            GenericAudioBufferRef::U16(b) => append!(b),
            GenericAudioBufferRef::U24(b) => append!(b),
            GenericAudioBufferRef::U32(b) => append!(b),
            GenericAudioBufferRef::S8(b) => append!(b),
            GenericAudioBufferRef::S16(b) => append!(b),
            GenericAudioBufferRef::S24(b) => append!(b),
            GenericAudioBufferRef::S32(b) => append!(b),
            GenericAudioBufferRef::F32(b) => append!(b),
            GenericAudioBufferRef::F64(b) => append!(b),
        }
    }
    if frames.is_empty() {
        return Err(fail());
    }
    Ok(StaticSoundData {
        sample_rate: rate,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    })
}
fn append_frames<S: Sample>(
    buffer: &AudioBuffer<S>,
    output: &mut Vec<Frame>,
    max: usize,
) -> Result<(), CoreError>
where
    f32: FromSample<S>,
{
    let fail = || {
        CoreError::invalid(
            "ASTRA_EMU_AUDIO_BOUND",
            "decoded audio format or size is unsupported",
        )
    };
    if !(1..=2).contains(&buffer.num_planes()) {
        return Err(fail());
    }
    let left = buffer.plane(0).ok_or_else(fail)?;
    if output.len().checked_add(left.len()).is_none_or(|n| n > max) {
        return Err(fail());
    }
    let right = if buffer.num_planes() == 2 {
        buffer.plane(1).ok_or_else(fail)?
    } else {
        left
    };
    for (l, r) in left.iter().zip(right) {
        let l: f32 = (*l).into_sample();
        let r: f32 = (*r).into_sample();
        if !l.is_finite() || !r.is_finite() {
            return Err(fail());
        }
        output.push(Frame::new(l, r));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(44_u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(8000_u32.to_le_bytes());
        bytes.extend(16000_u32.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(8_u32.to_le_bytes());
        for sample in [0_i16, 16384, -16384, 0] {
            bytes.extend(sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn decodes_complete_mono_wave_and_enforces_frame_budget() {
        let stop = AtomicBool::new(false);
        let audio = decode_audio(wave(), 4, &stop).unwrap();
        assert_eq!(audio.sample_rate, 8000);
        assert_eq!(audio.frames.len(), 4);
        assert_eq!(audio.frames[1].left, 0.5);
        assert_eq!(audio.frames[1].right, 0.5);
        assert_eq!(
            decode_audio(wave(), 3, &stop).err().unwrap().code(),
            "ASTRA_EMU_AUDIO_BOUND"
        );
    }

    #[test]
    fn cancellation_precedes_parsing_and_corruption_is_an_error() {
        assert_eq!(
            decode_audio(Vec::new(), 4, &AtomicBool::new(true))
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_AUDIO_CANCELLED"
        );
        assert_eq!(
            decode_audio(b"private-input".to_vec(), 4, &AtomicBool::new(false))
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_AUDIO_DECODE"
        );
    }
}
