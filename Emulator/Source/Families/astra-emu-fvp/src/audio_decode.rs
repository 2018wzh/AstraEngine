use rfvp::host_api::{
    AudioSampleFormat, AudioStreamDesc, EncodedAudioKind, RfvpError, SoftAudioVorbis,
};
use std::io::Cursor;

pub(super) struct SymphoniaBackend;

pub(super) struct DecoderState {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    had_samples: bool,
    pending: Vec<i16>,
}

impl SoftAudioVorbis for SymphoniaBackend {
    type Decoder = DecoderState;

    fn open(
        &mut self,
        bytes: &[u8],
    ) -> rfvp::host_api::RfvpResult<(Self::Decoder, AudioStreamDesc)> {
        self.open_with_kind(bytes, EncodedAudioKind::Unknown)
    }

    fn decode_interleaved_i16(
        &mut self,
        state: &mut Self::Decoder,
        out: &mut [i16],
    ) -> rfvp::host_api::RfvpResult<usize> {
        use symphonia::core::audio::SampleBuffer;

        while state.pending.len() < out.len() {
            let packet = match state.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break
                }
                Err(error) => return Err(map_symphonia_error(&error)),
            };
            if packet.track_id() != state.track_id {
                continue;
            }

            let decoded = state
                .decoder
                .decode(&packet)
                .map_err(|error| map_symphonia_error(&error))?;
            if decoded.spec().rate != state.sample_rate
                || decoded.spec().channels.count() as u16 != state.channels
            {
                return Err(RfvpError::InvalidData);
            }
            let mut samples = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            state.had_samples |= !samples.samples().is_empty();
            state.pending.extend_from_slice(samples.samples());
        }

        let count = out.len().min(state.pending.len());
        out[..count].copy_from_slice(&state.pending[..count]);
        state.pending.drain(..count);
        Ok(count)
    }

    fn seek_start(&mut self, state: &mut Self::Decoder) -> rfvp::host_api::RfvpResult<()> {
        use symphonia::core::formats::{SeekMode, SeekTo};

        if !state.had_samples {
            return Err(RfvpError::InvalidData);
        }
        state
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::TimeStamp {
                    ts: 0,
                    track_id: state.track_id,
                },
            )
            .map_err(|error| map_symphonia_error(&error))?;
        state.decoder.reset();
        state.pending.clear();
        Ok(())
    }

    fn close(&mut self, _state: Self::Decoder) {}
}

impl SymphoniaBackend {
    fn open_with_kind(
        &mut self,
        bytes: &[u8],
        kind: EncodedAudioKind,
    ) -> rfvp::host_api::RfvpResult<(DecoderState, AudioStreamDesc)> {
        use symphonia::core::{
            codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
            meta::MetadataOptions, probe::Hint,
        };

        let source = Box::new(Cursor::new(bytes.to_vec()));
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        match kind {
            EncodedAudioKind::Wav => {
                hint.with_extension("wav");
            }
            EncodedAudioKind::Ogg => {
                hint.with_extension("ogg");
            }
            EncodedAudioKind::Mp3 => {
                hint.with_extension("mp3");
            }
            EncodedAudioKind::Flac => {
                hint.with_extension("flac");
            }
            EncodedAudioKind::Unknown => {
                if bytes.starts_with(b"RIFF") {
                    hint.with_extension("wav");
                } else if bytes.starts_with(b"OggS") {
                    hint.with_extension("ogg");
                } else if bytes.starts_with(b"fLaC") {
                    hint.with_extension("flac");
                }
            }
        };
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| map_symphonia_error(&error))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or(RfvpError::Unsupported)?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let sample_rate = params.sample_rate.ok_or(RfvpError::Unsupported)?;
        let channels = params.channels.ok_or(RfvpError::Unsupported)?.count() as u16;
        if channels == 0 || channels > 2 {
            return Err(RfvpError::Unsupported);
        }
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|error| map_symphonia_error(&error))?;
        let desc = AudioStreamDesc {
            sample_rate,
            channels,
            sample_format: AudioSampleFormat::I16,
        };
        Ok((
            DecoderState {
                format,
                decoder,
                track_id,
                sample_rate,
                channels,
                had_samples: false,
                pending: Vec::new(),
            },
            desc,
        ))
    }
}

fn map_symphonia_error(error: &symphonia::core::errors::Error) -> RfvpError {
    use symphonia::core::errors::Error;

    match error {
        Error::IoError(_) => RfvpError::Io,
        Error::DecodeError(_) => RfvpError::InvalidData,
        Error::SeekError(_) => RfvpError::Backend,
        Error::Unsupported(_) => RfvpError::Unsupported,
        Error::LimitError(_) => RfvpError::CapacityExceeded,
        Error::ResetRequired => RfvpError::Backend,
    }
}
