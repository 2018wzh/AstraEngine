use super::*;
use rfvp::host_api::EncodedAudioKind;

#[test]
fn configuration_failure_preserves_the_host_diagnostic() {
    use abi_stable::type_level::downcasting::TD_Opaque;
    use astra_emu_family_api::{AudioSink, AudioSink_TO, FfiFamilyResult};
    struct RejectedDevice;
    impl AudioSink for RejectedDevice {
        fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
            Err(FamilyError::invalid(
                "DEVICE_UNAVAILABLE",
                "no output device",
            ))
            .into()
        }
        fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
            panic!("configuration must fail before PCM is written")
        }
        fn is_cancelled(&self) -> bool {
            false
        }
        fn cancel(&self) -> FfiFamilyResult<()> {
            Ok(()).into()
        }
    }
    let sink = AudioSink_TO::from_value(RejectedDevice, TD_Opaque);
    let error = match AudioBridge::new(sink) {
        Ok(_) => panic!("missing device must reject opening"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "DEVICE_UNAVAILABLE");
    assert_eq!(error.message.as_str(), "no output device");
}

fn wav_pcm16(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
    let data_size = u32::try_from(std::mem::size_of_val(samples)).expect("test WAV fits in u32");
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[test]
fn mixer_reports_natural_eof_to_hosted_state() {
    let mut mixer = SoftAudioMixer::new(
        SymphoniaBackend,
        SoftAudioConfig {
            mix_frames: 4,
            ..SoftAudioConfig::default()
        },
    );
    let id = AudioStreamId::se(0);
    mixer
        .load_encoded(
            id,
            EncodedAudioKind::Wav,
            &wav_pcm16(&[1_000, -1_000], 2, OUTPUT.sample_rate),
        )
        .expect("test WAV loads");
    mixer
        .play(id, Default::default(), 0)
        .expect("test voice starts");
    assert!(mixer.is_playing(id));

    let mut output = vec![0_i16; 8];
    assert!(mixer.mix_next(&mut output).expect("test voice mixes"));
    assert!(!mixer.is_playing(id));
    assert!(!mixer.mix_next(&mut output).expect("idle mixer advances"));
}

#[test]
fn mixer_keeps_looping_voice_playing_after_buffer_wrap() {
    let mut mixer = SoftAudioMixer::new(
        SymphoniaBackend,
        SoftAudioConfig {
            mix_frames: 4,
            ..SoftAudioConfig::default()
        },
    );
    let id = AudioStreamId::bgm(0);
    mixer
        .load_encoded(
            id,
            EncodedAudioKind::Wav,
            &wav_pcm16(&[1_000, -1_000], 2, OUTPUT.sample_rate),
        )
        .expect("test WAV loads");
    mixer
        .play(
            id,
            rfvp::host_api::AudioParams {
                repeat: true,
                ..Default::default()
            },
            0,
        )
        .expect("looping test voice starts");

    let mut output = vec![0_i16; 8];
    assert!(mixer.mix_next(&mut output).expect("looping voice mixes"));
    assert!(mixer.is_playing(id));
}
