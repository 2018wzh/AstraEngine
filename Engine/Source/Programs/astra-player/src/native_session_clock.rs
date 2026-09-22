use std::time::{Duration, Instant};

pub(super) struct ActiveSessionClock {
    origin: Instant,
    paused_at: Option<Instant>,
    paused_duration: Duration,
}

impl ActiveSessionClock {
    pub(super) fn new(origin: Instant) -> Self {
        Self {
            origin,
            paused_at: None,
            paused_duration: Duration::ZERO,
        }
    }

    pub(super) fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub(super) fn pause(&mut self, now: Instant) {
        self.paused_at.get_or_insert(now);
    }

    pub(super) fn resume(&mut self, now: Instant) -> bool {
        if let Some(start) = self.paused_at.take() {
            self.paused_duration += now.saturating_duration_since(start);
            true
        } else {
            false
        }
    }

    pub(super) fn elapsed_ms(&self, now: Instant) -> u64 {
        self.paused_at
            .unwrap_or(now)
            .saturating_duration_since(self.origin)
            .saturating_sub(self.paused_duration)
            .as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_time_never_becomes_media_or_presentation_debt() {
        let start = Instant::now();
        let mut clock = ActiveSessionClock::new(start);
        clock.pause(start + Duration::from_secs(2));
        clock.pause(start + Duration::from_secs(3));
        assert_eq!(clock.elapsed_ms(start + Duration::from_secs(30)), 2000);
        assert!(clock.resume(start + Duration::from_secs(30)));
        assert!(!clock.resume(start + Duration::from_secs(31)));
        assert_eq!(clock.elapsed_ms(start + Duration::from_secs(31)), 3000);
        clock.pause(start + Duration::from_secs(32));
        assert!(clock.resume(start + Duration::from_secs(40)));
        assert_eq!(clock.elapsed_ms(start + Duration::from_secs(41)), 5000);
    }
}
