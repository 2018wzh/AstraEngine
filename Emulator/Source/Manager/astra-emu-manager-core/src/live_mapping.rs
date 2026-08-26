//! Shared Runtime live DTO to Legacy family command mapping, used by both the
//! Slint manager host and the headless CLI runner so the two hosts cannot
//! drift apart on live audio/video/wait semantics.

use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7, LegacyAudioSampleFormat,
    LegacyPcmBufferV7, LegacyTextureFormat, LegacyVideoCommandV1, LegacyVideoMode,
};
use astra_plugin_abi::{
    RuntimeLiveAudioCommand, RuntimeLiveAudioEncoding, RuntimeLiveAudioPacket,
    RuntimeLiveAudioSampleFormat, RuntimeLivePcmBuffer, RuntimeLiveTextureFormat,
    RuntimeLiveVideoCommand, RuntimeLiveVideoCommandKind, RuntimeLiveVideoMode, RuntimeLiveWait,
    RuntimeLiveWaitKind,
};

/// Resolution state for a runtime live wait token held by a host step loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingLiveWait {
    DueStep(u64),
    Input(Vec<String>),
    MediaFence(String),
    PresentationFence,
    ProviderCompletion,
}

pub fn legacy_live_audio_packet(packet: RuntimeLiveAudioPacket) -> LegacyAudioPacketV7 {
    LegacyAudioPacketV7 {
        sequence: packet.sequence,
        stream_id: packet.stream_id,
        sample_rate: packet.sample_rate,
        channels: packet.channels,
        pcm: match packet.pcm {
            RuntimeLivePcmBuffer::I16(samples) => LegacyPcmBufferV7::I16(samples),
            RuntimeLivePcmBuffer::F32(samples) => LegacyPcmBufferV7::F32(samples),
        },
    }
}

pub fn legacy_live_audio_command(command: RuntimeLiveAudioCommand) -> LegacyAudioCommandV1 {
    match command {
        RuntimeLiveAudioCommand::LoadResource {
            stream_id,
            encoding,
            resource_uri,
            ..
        } => LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding: match encoding {
                RuntimeLiveAudioEncoding::Unknown => LegacyAudioEncoding::Unknown,
                RuntimeLiveAudioEncoding::Wav => LegacyAudioEncoding::Wav,
                RuntimeLiveAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
                RuntimeLiveAudioEncoding::Mp3 => LegacyAudioEncoding::Mp3,
                RuntimeLiveAudioEncoding::Flac => LegacyAudioEncoding::Flac,
            },
            resource_uri,
        },
        RuntimeLiveAudioCommand::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
            ..
        } => LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format: match sample_format {
                RuntimeLiveAudioSampleFormat::I16 => LegacyAudioSampleFormat::I16,
                RuntimeLiveAudioSampleFormat::F32 => LegacyAudioSampleFormat::F32,
            },
        },
        RuntimeLiveAudioCommand::SubmitI16 {
            stream_id, samples, ..
        } => LegacyAudioCommandV1::SubmitI16 { stream_id, samples },
        RuntimeLiveAudioCommand::SubmitF32 {
            stream_id, samples, ..
        } => LegacyAudioCommandV1::SubmitF32 { stream_id, samples },
        RuntimeLiveAudioCommand::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
            ..
        } => LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        },
        RuntimeLiveAudioCommand::Stop {
            stream_id, fade_ms, ..
        } => LegacyAudioCommandV1::Stop { stream_id, fade_ms },
        RuntimeLiveAudioCommand::Pause { stream_id, .. } => {
            LegacyAudioCommandV1::Pause { stream_id }
        }
        RuntimeLiveAudioCommand::Resume { stream_id, .. } => {
            LegacyAudioCommandV1::Resume { stream_id }
        }
        RuntimeLiveAudioCommand::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
            ..
        } => LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        },
        RuntimeLiveAudioCommand::DestroyStream { stream_id, .. } => {
            LegacyAudioCommandV1::DestroyStream { stream_id }
        }
        RuntimeLiveAudioCommand::MasterVolume { volume, .. } => {
            LegacyAudioCommandV1::MasterVolume { volume }
        }
    }
}

pub fn legacy_live_video_command(command: RuntimeLiveVideoCommand) -> LegacyVideoCommandV1 {
    match command.command {
        RuntimeLiveVideoCommandKind::Play {
            playback_id,
            resource_uri,
            mode,
            stage_width,
            stage_height,
        } => LegacyVideoCommandV1::Play {
            playback_id,
            resource_uri,
            mode: match mode {
                RuntimeLiveVideoMode::ModalWithAudio => LegacyVideoMode::ModalWithAudio,
                RuntimeLiveVideoMode::LayerNoAudio => LegacyVideoMode::LayerNoAudio,
            },
            stage_width,
            stage_height,
        },
        RuntimeLiveVideoCommandKind::Stop { playback_id } => {
            LegacyVideoCommandV1::Stop { playback_id }
        }
    }
}

/// Maps the runtime live texture format onto the legacy family wire format.
pub fn legacy_texture_format(format: RuntimeLiveTextureFormat) -> LegacyTextureFormat {
    match format {
        RuntimeLiveTextureFormat::Rgba8 => LegacyTextureFormat::Rgba8,
        RuntimeLiveTextureFormat::LumaAlpha8 => LegacyTextureFormat::LumaAlpha8,
    }
}

/// Converts a runtime live wait into the host-side pending representation.
/// The millisecond-to-tick rounding is saturation-safe and equivalent to
/// ceiling division for positive tick deltas.
pub fn live_wait_condition(
    wait: RuntimeLiveWait,
    step: u64,
    delta_ns: u64,
) -> (String, PendingLiveWait) {
    let token_id = wait.token_id;
    let condition = match wait.kind {
        RuntimeLiveWaitKind::Frame { frames } => {
            PendingLiveWait::DueStep(step.saturating_add(u64::from(frames).max(1)))
        }
        RuntimeLiveWaitKind::Time { milliseconds } => {
            let ticks = u64::from(milliseconds)
                .saturating_mul(1_000_000)
                .saturating_add(delta_ns.saturating_sub(1))
                / delta_ns.max(1);
            PendingLiveWait::DueStep(step.saturating_add(ticks.max(1)))
        }
        RuntimeLiveWaitKind::Input { keys } => PendingLiveWait::Input(keys),
        RuntimeLiveWaitKind::MediaFence { media_id } => PendingLiveWait::MediaFence(media_id),
        RuntimeLiveWaitKind::PresentationFence { .. } => PendingLiveWait::PresentationFence,
        RuntimeLiveWaitKind::ProviderCompletion { .. } => PendingLiveWait::ProviderCompletion,
    };
    (token_id, condition)
}
