use std::time::{Duration, Instant};

/// Maximum number of fixed steps that a real-time host may consume while
/// catching up after a wakeup. Remaining debt stays on the original timeline
/// for the next wakeup; input and shutdown can run between bounded batches.
pub const MAX_FIXED_CATCH_UP_STEPS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedDeadlineScheduler {
    origin: Instant,
    next_deadline: Instant,
    step: Duration,
    fixed_step: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedDeadlineDue {
    pub steps: u32,
    pub first_step: u64,
    pub lateness: Duration,
}

impl FixedDeadlineScheduler {
    pub fn new(step: Duration) -> Result<Self, &'static str> {
        if step.is_zero() {
            return Err("ASTRA_FIXED_DEADLINE_STEP_ZERO");
        }
        let origin = Instant::now();
        Ok(Self {
            origin,
            next_deadline: origin,
            step,
            fixed_step: 0,
        })
    }

    pub fn with_origin(origin: Instant, step: Duration) -> Result<Self, &'static str> {
        if step.is_zero() {
            return Err("ASTRA_FIXED_DEADLINE_STEP_ZERO");
        }
        Ok(Self {
            origin,
            next_deadline: origin,
            step,
            fixed_step: 0,
        })
    }

    /// Starts a fixed-deadline timeline after an explicitly completed startup
    /// step. Native hosts use this only for the first frame, where creating
    /// the initial retained GPU resources is part of host readiness rather
    /// than steady-state scheduling. The completed step remains observable
    /// through the caller's startup timing/diagnostic; this method does not
    /// skip or rebase any steady-state tick.
    pub fn after_completed_step(
        step: Duration,
        completed_steps: u64,
    ) -> Result<Self, &'static str> {
        if step.is_zero() {
            return Err("ASTRA_FIXED_DEADLINE_STEP_ZERO");
        }
        let origin = Instant::now();
        Ok(Self {
            origin,
            next_deadline: origin
                .checked_add(step)
                .ok_or("ASTRA_FIXED_DEADLINE_ORIGIN_OVERFLOW")?,
            step,
            fixed_step: completed_steps,
        })
    }

    pub fn origin(&self) -> Instant {
        self.origin
    }

    pub fn step(&self) -> Duration {
        self.step
    }

    pub fn fixed_step(&self) -> u64 {
        self.fixed_step
    }

    pub fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    pub fn wait_duration(&self, now: Instant) -> Duration {
        self.next_deadline.saturating_duration_since(now)
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_deadline
    }

    /// Consume a bounded batch of due fixed steps without rebasing the timeline.
    /// The caller must execute the returned consecutive steps in order.
    pub fn consume_due(&mut self, now: Instant) -> Result<Option<FixedDeadlineDue>, &'static str> {
        if now < self.next_deadline {
            return Ok(None);
        }
        let elapsed = now.duration_since(self.next_deadline);
        let overdue_steps = elapsed.as_nanos() / self.step.as_nanos();
        let due_steps = overdue_steps.saturating_add(1);
        let steps = due_steps.min(u128::from(MAX_FIXED_CATCH_UP_STEPS)) as u32;
        let first_step = self
            .fixed_step
            .checked_add(1)
            .ok_or("ASTRA_FIXED_DEADLINE_STEP_OVERFLOW")?;
        let fixed_step = self
            .fixed_step
            .checked_add(u64::from(steps))
            .ok_or("ASTRA_FIXED_DEADLINE_STEP_OVERFLOW")?;
        let next_deadline = self
            .next_deadline
            .checked_add(
                self.step
                    .checked_mul(steps)
                    .ok_or("ASTRA_FIXED_DEADLINE_TIME_OVERFLOW")?,
            )
            .ok_or("ASTRA_FIXED_DEADLINE_TIME_OVERFLOW")?;
        let lateness = now.duration_since(self.next_deadline);
        self.fixed_step = fixed_step;
        self.next_deadline = next_deadline;
        Ok(Some(FixedDeadlineDue {
            steps,
            first_step,
            lateness,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_absolute_deadlines_without_drift() {
        let origin = Instant::now();
        let step = Duration::from_millis(16);
        let mut scheduler = FixedDeadlineScheduler::with_origin(origin, step).unwrap();
        let due = scheduler.consume_due(origin).unwrap().unwrap();
        assert_eq!(due.steps, 1);
        assert_eq!(scheduler.next_deadline(), origin + step);
        let due = scheduler.consume_due(origin + step * 3).unwrap().unwrap();
        assert_eq!(due.steps, 3);
        assert_eq!(scheduler.next_deadline(), origin + step * 4);
    }

    #[test]
    fn catches_up_in_bounded_batches_without_skipping_or_rebasing() {
        let origin = Instant::now();
        let step = Duration::from_millis(1);
        let mut scheduler = FixedDeadlineScheduler::with_origin(origin, step).unwrap();
        let now = origin + step * 13;
        for (first, steps) in [(1, 4), (5, 4), (9, 4), (13, 2)] {
            let due = scheduler.consume_due(now).unwrap().unwrap();
            assert_eq!(due.first_step, first);
            assert_eq!(due.steps, steps);
        }
        assert_eq!(scheduler.fixed_step(), 14);
        assert_eq!(scheduler.next_deadline(), origin + step * 14);
        assert!(scheduler.consume_due(now).unwrap().is_none());
    }

    #[test]
    fn starts_after_completed_startup_step_without_rebasing_later_deadlines() {
        let step = Duration::from_millis(16);
        let scheduler = FixedDeadlineScheduler::after_completed_step(step, 1).unwrap();
        assert_eq!(scheduler.fixed_step(), 1);
        assert!(scheduler.next_deadline() >= scheduler.origin() + step);
    }
}
