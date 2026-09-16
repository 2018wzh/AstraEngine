use astra_emu_manager_core::LoadedFamilyPlugin;
use std::sync::{Arc, Mutex};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};

struct Capture(Arc<Mutex<Vec<String>>>);

#[test]
#[ignore = "requires an explicitly built Family plugin binary"]
fn dynamic_library_layout_matches_manager() {
    use abi_stable::{
        abi_stability::abi_checking::{check_layout_compatibility_with_globals, CheckingGlobals},
        library::{lib_header_from_raw_library, RawLibrary},
        StableAbi,
    };
    let path = std::env::var_os("ASTRA_EMU_TEST_PLUGIN").expect("explicit plugin binary required");
    let library = std::mem::ManuallyDrop::new(
        RawLibrary::load_at(std::path::Path::new(&path)).expect("load test library"),
    );
    let header = unsafe { lib_header_from_raw_library(&library) }.expect("read library ABI header");
    if let Err(error) = check_layout_compatibility_with_globals(
        <astra_emu_family_api::AstraFamilyModuleRef as StableAbi>::LAYOUT,
        header.layout().expect("library layout"),
        &CheckingGlobals::new(),
    ) {
        panic!("Family layout mismatch: {error}");
    }
}

struct EventName(Option<String>);
impl Visit for EventName {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "event" {
            self.0 = Some(value.to_owned());
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() == "astra_emu::family" {
            let mut name = EventName(None);
            event.record(&mut name);
            self.0
                .lock()
                .unwrap()
                .push(name.0.expect("stable family event"));
        }
    }
}

#[test]
#[ignore = "requires an explicitly built Family plugin binary"]
fn dynamic_library_diagnostics_reach_manager_once_per_initialization() {
    let path = std::env::var_os("ASTRA_EMU_TEST_PLUGIN").expect("explicit plugin binary required");
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Capture(events.clone()));
    tracing::subscriber::with_default(subscriber, || {
        for count in 1..=3 {
            let plugin = LoadedFamilyPlugin::load(&path).unwrap();
            assert_eq!(
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|event| event.as_str() == "family.diagnostics.ready")
                    .count(),
                count
            );
            drop(plugin);
        }
    });
}
