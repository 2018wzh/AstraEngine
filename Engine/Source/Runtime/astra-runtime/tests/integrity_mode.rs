use astra_runtime::{
    RuntimeConfig, RuntimeWorld, SaveRequest, TickInput, TickIntegrityMode, TickRequest,
};
use std::sync::atomic::{AtomicUsize, Ordering};

static SERIALIZATIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
struct CountedPayload(u32);

impl serde::Serialize for CountedPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SERIALIZATIONS.fetch_add(1, Ordering::SeqCst);
        serializer.serialize_u32(self.0)
    }
}

#[test]
fn both_tick_modes_leave_component_encoding_to_the_save_boundary() {
    for mode in [TickIntegrityMode::Shipping, TickIntegrityMode::Evidence] {
        let mut world =
            RuntimeWorld::create_with_integrity(RuntimeConfig::default(), mode).unwrap();
        let actor = world.create_actor("typed", vec![]).unwrap();
        world
            .attach_component(actor, "test.counted", &CountedPayload(7))
            .unwrap();
        SERIALIZATIONS.store(0, Ordering::SeqCst);
        for fixed_step in 1..=3 {
            let report = world
                .tick(TickRequest::live(
                    TickInput {
                        fixed_step,
                        delta_ns: 16_666_667,
                        seed: 0,
                    },
                    vec![],
                ))
                .unwrap();
            assert_eq!(report.integrity_mode, mode);
            assert_eq!(report.step, fixed_step);
        }
        assert_eq!(SERIALIZATIONS.load(Ordering::SeqCst), 0);
        world.save(SaveRequest::default()).unwrap();
        assert!(SERIALIZATIONS.load(Ordering::SeqCst) > 0);
    }
}

#[test]
fn restore_reports_saved_step_and_seed_without_aggregate_hashes() {
    let config = RuntimeConfig {
        seed: 29,
        required_slots: vec![],
    };
    let mut world =
        RuntimeWorld::create_with_integrity(config, TickIntegrityMode::Shipping).unwrap();
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 29,
            },
            vec![],
        ))
        .unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    let before = world.snapshot().unwrap();
    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let report = loaded.load(saved).unwrap();
    assert_eq!(report.step, 1);
    assert_eq!(report.seed, 29);
    assert_eq!(loaded.snapshot().unwrap(), before);
}
