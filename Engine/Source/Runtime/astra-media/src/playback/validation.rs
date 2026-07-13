use super::{
    playback_error, AudioFramePacket, MediaError, MediaPlaybackConfig, MediaPlaybackSession,
};

pub(super) fn validate_video_queue(session: &MediaPlaybackSession) -> Result<(), MediaError> {
    let mut previous_sequence = None;
    let mut previous_end = None;
    for packet in &session.video_queue {
        if packet.generation != session.generation
            || packet.sequence == 0
            || previous_sequence
                .is_some_and(|sequence: u64| sequence.checked_add(1) != Some(packet.sequence))
            || packet.duration_us == 0
            || packet.width == 0
            || packet.height == 0
            || !safe_resource_id(&packet.resource_id)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "media playback snapshot contains an invalid video queue",
            ));
        }
        validate_packet_time(
            packet.pts_us,
            packet.duration_us,
            session.config.duration_us,
            previous_end,
        )?;
        previous_sequence = Some(packet.sequence);
        previous_end = Some(packet.pts_us + packet.duration_us);
    }
    let expected_next = previous_sequence
        .map(|sequence| sequence.checked_add(1))
        .unwrap_or(Some(1))
        .ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "media playback video sequence overflowed",
            )
        })?;
    if session.next_video_sequence == 0
        || (previous_sequence.is_some() && session.next_video_sequence != expected_next)
    {
        return Err(playback_error(
            "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
            "media playback next video sequence precedes its queued packets",
        ));
    }
    Ok(())
}

pub(super) fn validate_audio_queue(session: &MediaPlaybackSession) -> Result<(), MediaError> {
    let mut previous_sequence = None;
    let mut previous_end = None;
    for packet in &session.audio_queue {
        if packet.generation != session.generation
            || packet.sequence == 0
            || previous_sequence
                .is_some_and(|sequence: u64| sequence.checked_add(1) != Some(packet.sequence))
            || packet.duration_us == 0
            || packet.sample_rate == 0
            || packet.channels == 0
            || packet.channels > 8
            || !valid_audio_packet_duration(packet)
            || !safe_resource_id(&packet.resource_id)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "media playback snapshot contains an invalid audio queue",
            ));
        }
        validate_packet_time(
            packet.pts_us,
            packet.duration_us,
            session.config.duration_us,
            previous_end,
        )?;
        previous_sequence = Some(packet.sequence);
        previous_end = Some(packet.pts_us + packet.duration_us);
    }
    let expected_next = previous_sequence
        .map(|sequence| sequence.checked_add(1))
        .unwrap_or(Some(1))
        .ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "media playback audio sequence overflowed",
            )
        })?;
    if session.next_audio_sequence == 0
        || (previous_sequence.is_some() && session.next_audio_sequence != expected_next)
    {
        return Err(playback_error(
            "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
            "media playback next audio sequence precedes its queued packets",
        ));
    }
    Ok(())
}

pub(super) fn validate_config(config: &MediaPlaybackConfig) -> Result<(), MediaError> {
    if (!config.has_audio && !config.has_video)
        || config.duration_us == 0
        || config.duration_us > i64::MAX as u64
        || config.max_video_frames == 0
        || config.max_audio_packets == 0
        || config.max_tick_us == 0
        || config.max_audio_clock_jump_us == 0
        || config.max_video_lag_us == 0
    {
        return Err(playback_error(
            "ASTRA_MEDIA_PLAYBACK_CONFIG",
            "media playback tracks, duration, and every queue/clock budget must be non-zero",
        ));
    }
    Ok(())
}

pub(super) fn validate_packet_time(
    pts_us: u64,
    duration_us: u64,
    session_duration_us: u64,
    previous_end_us: Option<u64>,
) -> Result<(), MediaError> {
    let end = pts_us.checked_add(duration_us).ok_or_else(|| {
        playback_error(
            "ASTRA_MEDIA_PACKET_TIME",
            "media packet timestamp overflowed",
        )
    })?;
    if end > session_duration_us || previous_end_us.is_some_and(|previous| pts_us < previous) {
        return Err(playback_error(
            "ASTRA_MEDIA_PACKET_TIME",
            "media packets overlap, move backward, or exceed duration",
        ));
    }
    Ok(())
}

pub(super) fn safe_resource_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(super) fn valid_audio_packet_duration(packet: &AudioFramePacket) -> bool {
    u64::from(packet.sample_rate)
        .checked_sub(1)
        .and_then(|_| u64::from(packet.frame_count).checked_mul(1_000_000))
        .map(|value| value / u64::from(packet.sample_rate))
        == Some(packet.duration_us)
}
