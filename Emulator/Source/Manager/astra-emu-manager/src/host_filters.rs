use super::*;
use astra_emu_manager_core::FilterSettings;

pub(super) fn configure(
    renderer: &mut dyn AstraUnderlayRenderer,
    settings: &FilterSettings,
) -> Result<(), String> {
    settings.validate()?;
    let saved = &settings.configuration;
    let config = crate::effects::FilterConfiguration {
        preset: match saved.preset {
            astra_emu_manager_core::FilterPreset::None => crate::effects::FilterPreset::None,
            astra_emu_manager_core::FilterPreset::Scale => crate::effects::FilterPreset::Scale,
            astra_emu_manager_core::FilterPreset::Sharpen => crate::effects::FilterPreset::Sharpen,
            astra_emu_manager_core::FilterPreset::Anime4kRestoreUpscale => {
                crate::effects::FilterPreset::Anime4kRestoreUpscale
            }
        },
        scale: saved.scale,
        strength: saved.strength,
        parameters: saved.parameters.clone(),
    };
    renderer.configure_filter(&config, settings.source.as_deref())
}

type FilterChange<'a, C> = dyn Fn(&mut C, FilterApply<'_>) -> Result<ManagerViewModel, String> + 'a;

/// Run a filter change through the renderer; a failed parameter shows under its field.
fn wire<C: ManagerController, R: AstraUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) -> impl Fn(Option<&str>, &FilterChange<'_, C>) + Clone {
    let wiring = wiring.clone();
    let renderer = renderer.clone();
    move |field, change| {
        let action = |c: &mut C| {
            change(c, &mut |settings| configure(&mut *renderer.borrow_mut(), settings))
        };
        match field {
            Some(key) => wiring.run_field(FieldScope::Filter, key, action),
            None => wiring.run(action),
        }
        if let Some(window) = wiring.window.upgrade() {
            window.window().request_redraw();
        }
    }
}

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    let filters = window.global::<Filters>();
    let run = wire(wiring, renderer);
    let set_preset = run.clone();
    filters.on_set_preset(move |preset| {
        set_preset(None, &|c, apply| c.set_filter_preset(preset.as_str(), apply));
    });
    let set_parameter = run.clone();
    filters.on_field_changed(move |key, value| {
        set_parameter(Some(key.as_str()), &|c, apply| {
            c.set_filter_parameter(key.as_str(), value.as_str(), apply)
        });
    });
    let reset = run.clone();
    filters.on_reset_parameters(move || reset(None, &|c, apply| c.reset_filter_parameters(apply)));
    filters.on_load_source(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择外部画面效果（HLSL）")
            .add_filter("HLSL", &["hlsl"])
            .pick_file()
        else {
            return;
        };
        run(None, &|c, apply| c.load_filter_source(&path, apply));
    });
}
