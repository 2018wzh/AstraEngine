#[path = "audio.rs"]
mod audio;
#[path = "descriptor.rs"]
mod descriptor;
#[path = "frame.rs"]
mod frame;
#[path = "input.rs"]
mod input;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "text.rs"]
mod text;

pub use audio::*;
pub use descriptor::*;
pub use frame::*;
pub use input::*;
pub use lifecycle::*;
pub use text::*;

pub(crate) use lifecycle::{validate_dimensions, validate_window_size};

#[cfg(test)]
mod tests {
    use abi_stable::std_types::{RNone, ROption, RSome, RVec};

    use super::*;
    use crate::{FAMILY_ABI_FINGERPRINT, FAMILY_API_SCHEMA};

    fn descriptor(capabilities: Vec<FamilyCapability>) -> FamilyDescriptor {
        FamilyDescriptor {
            family_id: "fvp".into(),
            plugin_id: "astra.emu.fvp".into(),
            abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
            version: "0.1.0".into(),
            capabilities: capabilities.into(),
            supported_formats: vec!["fvp.hcb".into()].into(),
        }
    }

    #[test]
    fn descriptor_requires_cpu_frame_and_rejects_duplicates() {
        assert_eq!(
            descriptor(vec![]).validate().unwrap_err().code(),
            "ASTRA_EMU_FAMILY_CAPABILITIES"
        );
        assert_eq!(
            descriptor(vec![FamilyCapability::CpuFrame, FamilyCapability::CpuFrame])
                .validate()
                .unwrap_err()
                .code(),
            "ASTRA_EMU_FAMILY_DUPLICATE_CAPABILITY"
        );
        descriptor(vec![FamilyCapability::CpuFrame])
            .validate()
            .unwrap();
    }

    #[test]
    fn probe_no_match_is_a_normal_result_and_game_id_accepts_unicode() {
        let no_match: ROption<ProbeReport> = RNone;
        assert!(no_match.is_none());
        ProbeReport {
            family_id: "fvp".into(),
            game_id: "樱花萌放".into(),
            format: "fvp.hcb".into(),
            confidence_permyriad: 10_000,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn frame_view_is_synchronous_and_checks_actual_bytes() {
        let info = FrameInfo {
            width: 2,
            height: 2,
            stride: 8,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        };
        assert_eq!(info.required_bytes(), Some(16));
        assert_eq!(
            FrameView::from_slice(&[0_u8; 15], info).unwrap_err().code(),
            "ASTRA_EMU_FAMILY_FRAME_BYTES"
        );
        let view = FrameView::from_slice(&[1_u8; 16], info).unwrap();
        assert_eq!(view.as_slice().len(), 16);
    }

    #[test]
    fn pcm_validation_rejects_non_finite_float_and_bad_alignment() {
        let format = PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::F32,
        };
        assert_eq!(
            PcmChunk::F32(vec![f32::NAN, 0.0].into())
                .validate(format)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_FAMILY_PCM_VALUE"
        );
        assert_eq!(
            PcmChunk::F32(vec![0.0].into())
                .validate(format)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_FAMILY_PCM_ALIGNMENT"
        );
    }

    #[test]
    fn open_audio_declaration_requires_explicit_format() {
        let response = OpenResponse {
            session_id: "session".into(),
            frame: FrameInfo {
                width: 1,
                height: 1,
                stride: 4,
                format: FrameFormat::Rgba8Srgb {
                    alpha: FrameAlpha::Opaque,
                },
            },
            audio_format: RNone,
        };
        assert_eq!(
            response
                .validate_for_descriptor(&descriptor(vec![
                    FamilyCapability::CpuFrame,
                    FamilyCapability::PcmAudio
                ]))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_FAMILY_AUDIO_DECLARATION"
        );
        let configured = OpenResponse {
            audio_format: RSome(PcmFormatSpec {
                sample_rate: 48_000,
                channels: 2,
                format: PcmFormat::I16,
            }),
            ..response
        };
        assert_eq!(
            configured
                .validate_for_descriptor(&descriptor(vec![FamilyCapability::CpuFrame]))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_FAMILY_AUDIO_DECLARATION"
        );
    }

    #[test]
    fn elapsed_zero_is_valid_and_text_poll_is_typed() {
        AdvanceRequest {
            session_id: "session".into(),
            elapsed_ns: 0,
            events: RVec::new(),
        }
        .validate()
        .unwrap();
        assert!(matches!(TextPollResult::Pending, TextPollResult::Pending));
        assert!(matches!(
            TextPollResult::Cancelled,
            TextPollResult::Cancelled
        ));
        assert_eq!(FAMILY_API_SCHEMA, "astra.emu.independent_family_api.v1");
    }
}
