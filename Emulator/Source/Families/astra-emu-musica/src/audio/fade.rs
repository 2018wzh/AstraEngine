use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FadeSnapshot {
    pub from_db: f32,
    pub to_db: f32,
    pub frames: u64,
    pub elapsed: u64,
    pub stop_after: bool,
}
impl FadeSnapshot {
    pub(super) fn new(from_db: f32, to_db: f32, ms: u32, stop_after: bool) -> Option<Self> {
        (ms != 0).then_some(Self {
            from_db,
            to_db,
            frames: u64::from(ms) * u64::from(FORMAT.sample_rate) / 1000,
            elapsed: 0,
            stop_after,
        })
    }
    pub(super) fn gain(&self) -> f32 {
        self.from_db
            + (self.to_db - self.from_db) * (self.elapsed as f64 / self.frames as f64) as f32
    }
    pub(super) fn remaining_tween(&self) -> Tween {
        Tween {
            duration: Duration::from_secs_f64(
                (self.frames - self.elapsed) as f64 / f64::from(FORMAT.sample_rate),
            ),
            ..Default::default()
        }
    }
    pub(super) fn validate(&self, playing: bool) -> FamilyResult<()> {
        if !playing
            || self.frames == 0
            || self.elapsed >= self.frames
            || self.frames > u64::from(u32::MAX) * u64::from(FORMAT.sample_rate) / 1000
            || !self.from_db.is_finite()
            || !self.to_db.is_finite()
            || !(Decibels::SILENCE.0..=0.0).contains(&self.from_db)
            || !(Decibels::SILENCE.0..=0.0).contains(&self.to_db)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_FADE",
                "snapshot fade is invalid",
            ));
        }
        Ok(())
    }
}
