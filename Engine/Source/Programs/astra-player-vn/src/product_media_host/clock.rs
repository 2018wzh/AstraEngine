use super::{media_error, PlatformError};

#[derive(Default)]
pub(super) struct PlaybackClock {
    playback_time_ms: u64,
    host_time_ms: Option<u64>,
}

impl PlaybackClock {
    pub(super) fn restored(playback_time_ms: u64) -> Self {
        Self {
            playback_time_ms,
            host_time_ms: None,
        }
    }

    pub(super) fn time_ms(&self) -> u64 {
        self.playback_time_ms
    }

    pub(super) fn advance(&mut self, host_time_ms: u64) -> Result<u64, PlatformError> {
        let delta = match self.host_time_ms {
            Some(previous) => host_time_ms.checked_sub(previous).ok_or_else(|| {
                media_error("player.media.clock", "ASTRA_PLAYER_MEDIA_CLOCK_REGRESSION")
            })?,
            None => 0,
        };
        let next = self.playback_time_ms.checked_add(delta).ok_or_else(|| {
            media_error("player.media.clock", "ASTRA_PLAYER_MEDIA_CLOCK_OVERFLOW")
        })?;
        self.playback_time_ms = next;
        self.host_time_ms = Some(host_time_ms);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeVnProductMediaHost;
    use astra_player_core::{PlayerTimelineTask, PlayerTimelineTaskAction};

    #[test]
    fn restored_timers_keep_remaining_duration_across_new_host_clocks() {
        for host_epoch in [0, 1_000_000] {
            let mut media = NativeVnProductMediaHost::default();
            let now = media.clock.advance(200).unwrap();
            media
                .timeline
                .schedule(
                    PlayerTimelineTask {
                        schema: "astra.player_timeline_task.v1".into(),
                        task_id: "wait".into(),
                        target: Some("scene".into()),
                        action: PlayerTimelineTaskAction::Start,
                        duration_ms: Some(100),
                        fence: Some("done".into()),
                    },
                    now,
                )
                .unwrap();
            let now = media.clock.advance(240).unwrap();
            assert!(media.timeline.poll(now).unwrap().is_empty());
            let snapshot = media.snapshot();
            media.clock.advance(5_000).unwrap();
            media.restore(snapshot).unwrap();
            let now = media.clock.advance(host_epoch).unwrap();
            assert_eq!(now, 40);
            assert!(media.timeline.poll(now).unwrap().is_empty());
            let now = media.clock.advance(host_epoch + 59).unwrap();
            assert!(media.timeline.poll(now).unwrap().is_empty());
            let now = media.clock.advance(host_epoch + 60).unwrap();
            assert_eq!(media.timeline.poll(now).unwrap().len(), 1);
            assert!(media.timeline.poll(now).unwrap().is_empty());
        }
    }

    #[test]
    fn clock_errors_do_not_advance_either_clock() {
        let mut clock = PlaybackClock::restored(u64::MAX - 1);
        assert_eq!(clock.advance(100).unwrap(), u64::MAX - 1);
        assert!(clock
            .advance(99)
            .unwrap_err()
            .to_string()
            .contains("REGRESSION"));
        assert!(clock
            .advance(102)
            .unwrap_err()
            .to_string()
            .contains("OVERFLOW"));
        assert_eq!(clock.advance(101).unwrap(), u64::MAX);
    }
}
