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

fn apply(
    controller: &mut dyn ManagerController,
    renderer: &mut dyn AstraUnderlayRenderer,
) -> Result<ManagerViewModel, String> {
    let previous = controller.filter_settings();
    let candidate = controller.pending_filter_settings()?;
    configure(renderer, &candidate)?;
    match controller.commit_filter_settings(candidate) {
        Ok(model) => Ok(model),
        Err(error) => {
            configure(renderer, &previous).map_err(|rollback| format!("{error}; {rollback}"))?;
            Err(error)
        }
    }
}

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) {
    let c = controller.clone();
    let r = renderer.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_set_filter_preset(move |preset| {
        let result = (|| {
            c.borrow_mut()
                .filter_config_changed("filter.preset", preset.as_str())?;
            apply(&mut *c.borrow_mut(), &mut *r.borrow_mut())
        })();
        if let Some(window) = w.upgrade() {
            match result {
                Ok(model) => {
                    apply_model(&a, &model);
                    window.window().request_redraw();
                }
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let c = controller.clone();
    let w = adapter.window().as_weak();
    adapter
        .window()
        .on_filter_config_changed(move |key, value| {
            if let Err(error) = c
                .borrow_mut()
                .filter_config_changed(key.as_str(), value.as_str())
            {
                if let Some(window) = w.upgrade() {
                    window.set_global_diagnostic(error.into());
                }
            }
        });
    let c = controller.clone();
    let r = renderer.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_save_filter_config(move || {
        let result = apply(&mut *c.borrow_mut(), &mut *r.borrow_mut());
        if let Some(window) = w.upgrade() {
            match result {
                Ok(model) => {
                    apply_model(&a, &model);
                    window.window().request_redraw();
                }
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_reset_filter_config(move || {
        let result = c.borrow_mut().reset_filter_config();
        if let Some(window) = w.upgrade() {
            match result {
                Ok(model) => apply_model(&a, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_load_filter_source(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HLSL", &["hlsl"])
            .pick_file()
        else {
            return;
        };
        let result = (|| {
            use std::io::Read;
            let file = std::fs::File::open(path).map_err(|_| "ASTRA_EMU_FILTER_SOURCE_OPEN")?;
            let mut bytes = Vec::new();
            file.take(1_048_577)
                .read_to_end(&mut bytes)
                .map_err(|_| "ASTRA_EMU_FILTER_SOURCE_READ")?;
            if bytes.len() > 1_048_576 {
                return Err("ASTRA_EMU_FILTER_SOURCE_SIZE".into());
            }
            let source = String::from_utf8(bytes).map_err(|_| "ASTRA_EMU_FILTER_SOURCE_UTF8")?;
            c.borrow_mut()
                .filter_config_changed("filter.source", &source)?;
            c.borrow().model()
        })();
        if let Some(window) = w.upgrade() {
            match result {
                Ok(model) => apply_model(&a, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
}
