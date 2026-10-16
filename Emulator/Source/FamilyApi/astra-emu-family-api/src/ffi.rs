#[path = "config.rs"]
mod config;
pub use config::*;
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
#[path = "modal.rs"]
mod modal;
#[path = "text.rs"]
mod text;

pub use audio::*;
pub use descriptor::*;
pub use frame::*;
pub use input::*;
pub use lifecycle::*;
pub use modal::*;
pub use text::*;

pub(crate) use lifecycle::{validate_dimensions, validate_window_size};

#[cfg(test)]
mod tests {
    use abi_stable::std_types::{RNone, ROption, RSome};

    use super::*;
    use crate::FAMILY_ABI_FINGERPRINT;

    fn descriptor(capabilities: Vec<FamilyCapability>) -> FamilyDescriptor {
        FamilyDescriptor {
            configuration: Default::default(),
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
                logical_width: 1,
                logical_height: 1,
                stride: 4,
                format: FrameFormat::Rgba8Srgb {
                    alpha: FrameAlpha::Opaque,
                },
                rows: FrameRowOrder::TopToBottom,
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
    fn pointer_updates_preserve_unmentioned_properties() {
        let mut request = FamilyPointerRequest {
            logical_x: ROption::RSome(10),
            visible: ROption::RSome(false),
            ..Default::default()
        };
        request.merge(FamilyPointerRequest {
            logical_y: ROption::RSome(20),
            autohide_ms: ROption::RSome(0),
            ..Default::default()
        });
        assert_eq!(request.logical_x, ROption::RSome(10));
        assert_eq!(request.logical_y, ROption::RSome(20));
        assert_eq!(request.visible, ROption::RSome(false));
        assert_eq!(request.autohide_ms, ROption::RSome(0));
        FamilyWindowCommand::SetPointer(request).validate().unwrap();
    }
}
