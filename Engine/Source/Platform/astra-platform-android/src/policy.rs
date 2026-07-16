use astra_platform::{PlatformError, PlatformErrorCode, PlatformHostProfile};

use crate::{ANDROID_AUDIO_BACKEND_MISMATCH, ANDROID_JIT_FORBIDDEN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidAudioBackend {
    AAudio,
    OpenSlEs,
}

impl AndroidAudioBackend {
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::AAudio => "oboe_aaudio",
            Self::OpenSlEs => "oboe_opensl_es",
        }
    }
}

pub fn validate_selected_audio_backend(
    profile: &PlatformHostProfile,
    selected: AndroidAudioBackend,
) -> Result<(), PlatformError> {
    let position = profile
        .audio
        .providers
        .iter()
        .position(|provider| provider == selected.provider_id())
        .ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "audio.backend.select",
                "actual Android audio backend is not declared by the profile",
            )
            .with_field("diagnostic_code", ANDROID_AUDIO_BACKEND_MISMATCH)
        })?;
    if selected == AndroidAudioBackend::OpenSlEs && position == 0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "audio.backend.select",
            "OpenSL ES must be an explicit compatibility fallback after AAudio",
        )
        .with_field("diagnostic_code", ANDROID_AUDIO_BACKEND_MISMATCH));
    }
    Ok(())
}

pub fn validate_interpreter_only_features<'a>(
    features: impl IntoIterator<Item = &'a str>,
) -> Result<(), PlatformError> {
    if features.into_iter().any(|feature| {
        let normalized = feature.to_ascii_lowercase();
        normalized == "jit" || normalized.ends_with("-jit") || normalized.contains("luau_jit")
    }) {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "android.bundle.features",
            "Android Player requires interpreter-only Luau",
        )
        .with_field("diagnostic_code", ANDROID_JIT_FORBIDDEN));
    }
    Ok(())
}
