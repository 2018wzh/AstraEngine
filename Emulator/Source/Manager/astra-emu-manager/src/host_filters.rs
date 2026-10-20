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

fn wire<C: ManagerController, R: AstraUnderlayRenderer>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) -> impl Fn(&FilterChange<'_, C>) + Clone {
    let c = controller.clone();
    let r = renderer.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    move |change| {
        let result = change(&mut *c.borrow_mut(), &mut |settings| {
            configure(&mut *r.borrow_mut(), settings)
        });
        if let Some(window) = w.upgrade() {
            match result {
                Ok(model) => {
                    apply_model(&a, &model);
                    window.window().request_redraw();
                }
                Err(error) => report_error(&window, &error),
            }
        }
    }
}

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) {
    let filters = adapter.window().global::<Filters>();
    let run = wire(adapter, controller, renderer);
    let set_preset = run.clone();
    filters.on_set_preset(move |preset| {
        set_preset(&|c, apply| c.set_filter_preset(preset.as_str(), apply));
    });
    let set_parameter = run.clone();
    filters.on_field_changed(move |key, value| {
        set_parameter(&|c, apply| c.set_filter_parameter(key.as_str(), value.as_str(), apply));
    });
    let reset = run.clone();
    filters.on_reset(move || reset(&|c, apply| c.reset_filter_parameters(apply)));
    filters.on_load_source(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HLSL", &["hlsl"])
            .pick_file()
        else {
            return;
        };
        run(&|c, apply| c.load_filter_source(&path, apply));
    });
}
