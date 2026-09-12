use crate::scene::error;
use astra_emu_family_api::FamilyResult;
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

pub(crate) fn decode(
    bytes: Vec<u8>,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> FamilyResult<StaticSoundData> {
    let fail = || {
        error(
            "ASTRA_EMU_MINORI_AUDIO_DECODE",
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
            return Err(error(
                "ASTRA_EMU_MINORI_AUDIO_CANCELLED",
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
) -> FamilyResult<()>
where
    f32: FromSample<S>,
{
    let fail = || {
        error(
            "ASTRA_EMU_MINORI_AUDIO_BOUND",
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
