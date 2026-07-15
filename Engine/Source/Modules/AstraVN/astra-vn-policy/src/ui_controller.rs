use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use astra_policy::{create_sandboxed_lua, PolicyExecutionBudget};
use astra_ui_core::{UiValue, MAX_EFFECTS_PER_CALL};
use astra_vn_ui::{
    VnUiAction, VnUiControllerEffect, VnUiControllerManifest, VnUiControllerSnapshot,
    VnUiSessionState,
};
use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LuauUiControllerError {
    #[error("ASTRA_VN_UI_CONTROLLER_DUPLICATE: controller id is already registered")]
    Duplicate,
    #[error("ASTRA_VN_UI_CONTROLLER_MISSING: controller id is not registered")]
    Missing,
    #[error("ASTRA_VN_UI_CONTROLLER_REGISTRATION: {0}")]
    Registration(String),
    #[error("ASTRA_VN_UI_CONTROLLER_RUNTIME: {0}")]
    Runtime(String),
    #[error("ASTRA_VN_UI_CONTROLLER_EFFECT: {0}")]
    Effect(String),
}

struct RegisteredController {
    manifest: VnUiControllerManifest,
    source: String,
}

/// Deterministic host for project-authored UI controllers.
///
/// Each callback receives a fresh capability sandbox. Session state is held by
/// Rust, so Lua functions, threads and userdata can never enter save/replay.
pub struct LuauUiControllerHost {
    budget: PolicyExecutionBudget,
    controllers: BTreeMap<String, RegisteredController>,
}

impl LuauUiControllerHost {
    pub fn with_default_budget() -> Result<Self, LuauUiControllerError> {
        Self::new(PolicyExecutionBudget::default())
    }

    pub fn new(budget: PolicyExecutionBudget) -> Result<Self, LuauUiControllerError> {
        budget
            .validate()
            .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
        Ok(Self {
            budget,
            controllers: BTreeMap::new(),
        })
    }

    pub fn register_source(
        &mut self,
        source: impl Into<String>,
    ) -> Result<(), LuauUiControllerError> {
        let source = source.into();
        let (_, registrations) = load_controller_source(&source, self.budget)?;
        if registrations.is_empty() {
            return Err(LuauUiControllerError::Registration(
                "a controller source must register at least one controller".into(),
            ));
        }
        for manifest in registrations.values() {
            manifest
                .validate()
                .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
            if self.controllers.contains_key(&manifest.id) {
                return Err(LuauUiControllerError::Duplicate);
            }
        }
        self.controllers
            .extend(registrations.into_iter().map(|(id, manifest)| {
                (
                    id,
                    RegisteredController {
                        manifest,
                        source: source.clone(),
                    },
                )
            }));
        Ok(())
    }

    pub fn manifest(&self, id: &str) -> Option<&VnUiControllerManifest> {
        self.controllers.get(id).map(|entry| &entry.manifest)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &VnUiControllerManifest> {
        self.controllers.values().map(|entry| &entry.manifest)
    }

    pub fn source(&self, id: &str) -> Option<&str> {
        self.controllers.get(id).map(|entry| entry.source.as_str())
    }

    pub fn invoke_action(
        &self,
        id: &str,
        model_schema: &str,
        model: &UiValue,
        action: &VnUiAction,
        session: &mut VnUiSessionState,
    ) -> Result<Vec<VnUiControllerEffect>, LuauUiControllerError> {
        let registered = self
            .controllers
            .get(id)
            .ok_or(LuauUiControllerError::Missing)?;
        if registered.manifest.model_schema != model_schema {
            return Err(LuauUiControllerError::Runtime(
                "controller model schema does not match the bound view model".into(),
            ));
        }
        let (lua, registrations) = load_controller_source(&registered.source, self.budget)?;
        let loaded = registrations.get(id).ok_or_else(|| {
            LuauUiControllerError::Registration(
                "controller source did not reproduce its registered identity".into(),
            )
        })?;
        if loaded != &registered.manifest {
            return Err(LuauUiControllerError::Registration(
                "controller manifest changed between registration and invocation".into(),
            ));
        }
        let registry: Table = lua
            .globals()
            .get("__astra_ui_controllers")
            .map_err(runtime_error)?;
        let handlers: Table = registry.get(id).map_err(runtime_error)?;
        let handler: Function = handlers.get("on_action").map_err(|_| {
            LuauUiControllerError::Registration("controller requires an on_action handler".into())
        })?;
        let context = lua.create_table().map_err(runtime_error)?;
        context
            .set(
                "state",
                lua.to_value(session.values()).map_err(runtime_error)?,
            )
            .map_err(runtime_error)?;
        let output: Value = handler
            .call((
                context,
                lua.to_value(model).map_err(runtime_error)?,
                lua.to_value(action).map_err(runtime_error)?,
            ))
            .map_err(runtime_error)?;
        let effects: Vec<VnUiControllerEffect> = lua.from_value(output).map_err(|error| {
            LuauUiControllerError::Effect(format!(
                "controller returned an unserializable effect list: {error}"
            ))
        })?;
        validate_effects(&registered.manifest, &effects)?;
        session
            .apply(&effects)
            .map_err(|error| LuauUiControllerError::Effect(error.to_string()))?;
        Ok(effects)
    }
}

fn load_controller_source(
    source: &str,
    budget: PolicyExecutionBudget,
) -> Result<(Lua, BTreeMap<String, VnUiControllerManifest>), LuauUiControllerError> {
    let lua = create_sandboxed_lua(budget)
        .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
    let registrations = Rc::new(RefCell::new(BTreeMap::new()));
    install_ui_api(&lua, Rc::clone(&registrations))?;
    lua.load(source)
        .set_name("astra-ui-controller")
        .exec()
        .map_err(runtime_error)?;
    let registrations = registrations.borrow().clone();
    Ok((lua, registrations))
}

fn install_ui_api(
    lua: &Lua,
    registrations: Rc<RefCell<BTreeMap<String, VnUiControllerManifest>>>,
) -> Result<(), LuauUiControllerError> {
    let registry = lua.create_table().map_err(runtime_error)?;
    lua.globals()
        .set("__astra_ui_controllers", registry.clone())
        .map_err(runtime_error)?;
    let register_registry = registry;
    let register = lua
        .create_function(
            move |lua, (id, manifest, handlers): (String, Table, Table)| {
                let snapshot = lua.from_value(manifest.get::<Value>("snapshot")?)?;
                let manifest = VnUiControllerManifest {
                    schema: manifest.get("schema")?,
                    id: id.clone(),
                    view: manifest.get("view")?,
                    model_schema: manifest.get("model_schema")?,
                    snapshot,
                };
                manifest
                    .validate()
                    .map_err(|error| mlua::Error::runtime(error.to_string()))?;
                let _: Function = handlers.get("on_action")?;
                if registrations
                    .borrow_mut()
                    .insert(id.clone(), manifest)
                    .is_some()
                {
                    return Err(mlua::Error::runtime(
                        "ASTRA_VN_UI_CONTROLLER_DUPLICATE: duplicate controller registration",
                    ));
                }
                register_registry.set(id, handlers)?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    let controller = lua.create_table().map_err(runtime_error)?;
    controller
        .set("register", register)
        .map_err(runtime_error)?;
    let ui = lua.create_table().map_err(runtime_error)?;
    ui.set("controller", controller).map_err(runtime_error)?;
    let effect = lua.create_table().map_err(runtime_error)?;
    effect
        .set(
            "forward",
            lua.create_function(|lua, action: Value| {
                let action: VnUiAction = lua.from_value(action)?;
                lua.to_value(&VnUiControllerEffect::Forward { action })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "open_modal",
            lua.create_function(|lua, (view_id, model): (String, Value)| {
                let model: UiValue = lua.from_value(model)?;
                lua.to_value(&VnUiControllerEffect::OpenModal { view_id, model })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "close_modal",
            lua.create_function(|lua, ()| lua.to_value(&VnUiControllerEffect::CloseModal))
                .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "focus",
            lua.create_function(|lua, semantic_id: String| {
                lua.to_value(&VnUiControllerEffect::Focus { semantic_id })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "set_session_state",
            lua.create_function(|lua, (key, value): (String, Value)| {
                let value: UiValue = lua.from_value(value)?;
                lua.to_value(&VnUiControllerEffect::SetSessionState { key, value })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "animation",
            lua.create_function(|lua, (target_id, preset_id): (String, String)| {
                lua.to_value(&VnUiControllerEffect::Animation {
                    target_id,
                    preset_id,
                })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    effect
        .set(
            "trace",
            lua.create_function(|lua, (event, fields): (String, Value)| {
                let fields: BTreeMap<String, String> = lua.from_value(fields)?;
                lua.to_value(&VnUiControllerEffect::Trace { event, fields })
            })
            .map_err(runtime_error)?,
        )
        .map_err(runtime_error)?;
    ui.set("effect", effect).map_err(runtime_error)?;
    let astra = lua.create_table().map_err(runtime_error)?;
    astra.set("ui", ui).map_err(runtime_error)?;
    lua.globals().set("astra", astra).map_err(runtime_error)
}

fn validate_effects(
    manifest: &VnUiControllerManifest,
    effects: &[VnUiControllerEffect],
) -> Result<(), LuauUiControllerError> {
    if effects.len() > MAX_EFFECTS_PER_CALL {
        return Err(LuauUiControllerError::Effect(
            "controller exceeded the effect count limit".into(),
        ));
    }
    if manifest.snapshot == VnUiControllerSnapshot::None
        && effects
            .iter()
            .any(|effect| matches!(effect, VnUiControllerEffect::SetSessionState { .. }))
    {
        return Err(LuauUiControllerError::Effect(
            "snapshot=none controller attempted to write session state".into(),
        ));
    }
    for effect in effects {
        if let VnUiControllerEffect::Trace { event, fields } = effect {
            if event.trim().is_empty()
                || fields.values().any(|value| {
                    value.contains(['/', '\\'])
                        || value.contains("payload")
                        || value.contains("content")
                })
            {
                return Err(LuauUiControllerError::Effect(
                    "controller trace contains an unsafe or non-redacted field".into(),
                ));
            }
        }
    }
    Ok(())
}

fn runtime_error(error: mlua::Error) -> LuauUiControllerError {
    LuauUiControllerError::Runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[astra_headless_test::test]
    fn controller_runs_in_fresh_sandbox_and_returns_typed_effects() {
        let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
        host.register_source(
            r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.close_modal() }
    end,
})
"#,
        )
        .expect("register");
        let mut session = VnUiSessionState::default();
        let effects = host
            .invoke_action(
                "controller.test",
                "astra.vn.ui_model.test.v1",
                &UiValue::Map(BTreeMap::new()),
                &VnUiAction::Advance,
                &mut session,
            )
            .expect("invoke");
        assert_eq!(effects, vec![VnUiControllerEffect::CloseModal]);
        assert!(session.values().is_empty());
    }

    #[astra_headless_test::test]
    fn snapshot_none_controller_cannot_write_session_state() {
        let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
        host.register_source(
            r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.set_session_state("open", { bool = true }) }
    end,
})
"#,
        )
        .expect("register");
        let error = host
            .invoke_action(
                "controller.test",
                "astra.vn.ui_model.test.v1",
                &UiValue::Map(BTreeMap::new()),
                &VnUiAction::Advance,
                &mut VnUiSessionState::default(),
            )
            .expect_err("authority violation must fail");
        assert!(error.to_string().contains("snapshot=none"));
    }
}
