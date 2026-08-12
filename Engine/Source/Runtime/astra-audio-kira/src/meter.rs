use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc,
};

use kira::{
    effect::{Effect, EffectBuilder},
    info::Info,
    Frame,
};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MasterMixTelemetry {
    pub peak_amplitude: f32,
    pub overload_frames: u64,
    pub rendered_frames: u64,
}

#[derive(Default)]
struct MasterMixAtomics {
    peak_amplitude_bits: AtomicU32,
    overload_frames: AtomicU64,
    rendered_frames: AtomicU64,
}

#[derive(Clone, Default)]
pub struct MasterMixMeterHandle(Arc<MasterMixAtomics>);

impl MasterMixMeterHandle {
    #[must_use]
    pub fn telemetry(&self) -> MasterMixTelemetry {
        MasterMixTelemetry {
            peak_amplitude: f32::from_bits(self.0.peak_amplitude_bits.load(Ordering::Relaxed)),
            overload_frames: self.0.overload_frames.load(Ordering::Relaxed),
            rendered_frames: self.0.rendered_frames.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MasterMixMeterBuilder;

impl EffectBuilder for MasterMixMeterBuilder {
    type Handle = MasterMixMeterHandle;

    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        let shared = Arc::new(MasterMixAtomics::default());
        (
            Box::new(MasterMixMeter {
                shared: Arc::clone(&shared),
            }),
            MasterMixMeterHandle(shared),
        )
    }
}

struct MasterMixMeter {
    shared: Arc<MasterMixAtomics>,
}

impl Effect for MasterMixMeter {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let mut peak = 0.0_f32;
        let mut overload_frames = 0_u64;
        for frame in input.iter() {
            let frame_peak = frame.left.abs().max(frame.right.abs());
            peak = peak.max(frame_peak);
            overload_frames += u64::from(frame_peak > 1.0);
        }
        self.shared
            .peak_amplitude_bits
            .fetch_max(peak.to_bits(), Ordering::Relaxed);
        self.shared
            .overload_frames
            .fetch_add(overload_frames, Ordering::Relaxed);
        self.shared
            .rendered_frames
            .fetch_add(input.len() as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use kira::{effect::EffectBuilder, info::MockInfoBuilder, Frame};

    use super::MasterMixMeterBuilder;

    #[test]
    fn meter_distinguishes_full_scale_from_pre_master_overload() {
        let (mut effect, handle) = MasterMixMeterBuilder.build();
        let mut input = [Frame::new(-1.0, 0.5), Frame::new(1.25, -0.25)];
        let info = MockInfoBuilder::new().build();
        effect.process(&mut input, 1.0 / 48_000.0, &info);

        let telemetry = handle.telemetry();
        assert_eq!(telemetry.peak_amplitude, 1.25);
        assert_eq!(telemetry.overload_frames, 1);
        assert_eq!(telemetry.rendered_frames, 2);
        assert_eq!(input[0], Frame::new(-1.0, 0.5));
    }
}
