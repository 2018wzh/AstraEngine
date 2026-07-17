use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use astra_policy::{create_sandboxed_lua, PolicyExecutionBudget};
use astra_ui_core::{UiValue, MAX_EFFECTS_PER_CALL};
use astra_vn_ui::{
    VnUiAction, VnUiControllerEffect, VnUiControllerManifest, VnUiControllerSnapshot,
    VnUiControllerUpdate, VnUiSessionState,
};
#[cfg(feature = "portable-luau-runtime")]
use luaur_rt as mlua;
#[cfg(feature = "portable-luau-runtime")]
use luaur_rt::{Lua, LuaSerdeExt, Table, Value};
#[cfg(feature = "luau-runtime")]
use mlua::{Lua, LuaSerdeExt, Table, Value};
use thiserror::Error;

#[cfg(all(feature = "luau-runtime", feature = "portable-luau-runtime"))]
compile_error!("native and portable Luau runtimes are mutually exclusive");

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
        self.invoke(
            id,
            model_schema,
            model,
            ControllerInvocation::Action(action),
            session,
        )
    }

    pub fn invoke_open(
        &self,
        id: &str,
        model_schema: &str,
        model: &UiValue,
        session: &mut VnUiSessionState,
    ) -> Result<Vec<VnUiControllerEffect>, LuauUiControllerError> {
        self.invoke(id, model_schema, model, ControllerInvocation::Open, session)
    }

    pub fn invoke_update(
        &self,
        id: &str,
        model_schema: &str,
        model: &UiValue,
        update: &VnUiControllerUpdate,
        session: &mut VnUiSessionState,
    ) -> Result<Vec<VnUiControllerEffect>, LuauUiControllerError> {
        self.invoke(
            id,
            model_schema,
            model,
            ControllerInvocation::Update(update),
            session,
        )
    }

    fn invoke(
        &self,
        id: &str,
        model_schema: &str,
        model: &UiValue,
        invocation: ControllerInvocation<'_>,
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
        let (handler_name, required) = invocation.handler();
        let handler_value: Value = handlers.get(handler_name).map_err(runtime_error)?;
        let handler = match handler_value {
            Value::Function(handler) => handler,
            Value::Nil if !required => return Ok(Vec::new()),
            Value::Nil => {
                return Err(LuauUiControllerError::Registration(format!(
                    "controller requires an {handler_name} handler"
                )))
            }
            _ => {
                return Err(LuauUiControllerError::Registration(format!(
                    "controller {handler_name} must be a function"
                )))
            }
        };
        let context = create_table(&lua).map_err(runtime_error)?;
        context
            .set(
                "state",
                lua.to_value(session.values()).map_err(runtime_error)?,
            )
            .map_err(runtime_error)?;
        let model = lua.to_value(model).map_err(runtime_error)?;
        let output: Value = match invocation {
            ControllerInvocation::Open => handler.call((context, model)),
            ControllerInvocation::Action(action) => {
                handler.call((context, model, lua.to_value(action).map_err(runtime_error)?))
            }
            ControllerInvocation::Update(update) => {
                handler.call((context, model, lua.to_value(update).map_err(runtime_error)?))
            }
        }
        .map_err(runtime_error)?;
        let effects: Vec<VnUiControllerEffect> = lua.from_value(output).map_err(|error| {
            LuauUiControllerError::Effect(format!(
                "controller returned an unserializable effect list: {error}"
            ))
        })?;
        validate_effects(&registered.manifest, invocation, &effects)?;
        session
            .apply(&effects)
            .map_err(|error| LuauUiControllerError::Effect(error.to_string()))?;
        Ok(effects)
    }
}

#[derive(Clone, Copy)]
enum ControllerInvocation<'a> {
    Open,
    Action(&'a VnUiAction),
    Update(&'a VnUiControllerUpdate),
}

impl ControllerInvocation<'_> {
    fn handler(self) -> (&'static str, bool) {
        match self {
            Self::Open => ("on_open", false),
            Self::Action(_) => ("on_action", true),
            Self::Update(_) => ("on_update", false),
        }
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
    let registry = create_table(lua).map_err(runtime_error)?;
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
                validate_handler(&handlers, "on_action", true)?;
                validate_handler(&handlers, "on_open", false)?;
                validate_handler(&handlers, "on_update", false)?;
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
    let controller = create_table(lua).map_err(runtime_error)?;
    controller
        .set("register", register)
        .map_err(runtime_error)?;
    let ui = create_table(lua).map_err(runtime_error)?;
    ui.set("controller", controller).map_err(runtime_error)?;
    let effect = create_table(lua).map_err(runtime_error)?;
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
                let model = lua_value_to_ui_value(model, 0)?;
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
                let value = lua_value_to_ui_value(value, 0)?;
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
    let astra = create_table(lua).map_err(runtime_error)?;
    astra.set("ui", ui).map_err(runtime_error)?;
    lua.globals().set("astra", astra).map_err(runtime_error)
}

fn lua_value_to_ui_value(value: Value, depth: usize) -> mlua::Result<UiValue> {
    if depth > 16 {
        return Err(mlua::Error::runtime(
            "ASTRA_VN_UI_CONTROLLER_VALUE_DEPTH: UI value nesting exceeds 16",
        ));
    }
    match value {
        Value::Nil => Ok(UiValue::Null),
        Value::Boolean(value) => Ok(UiValue::Bool(value)),
        Value::Integer(value) => Ok(UiValue::Integer(i64::from(value))),
        Value::Number(value) if value.is_finite() => Ok(UiValue::Number(value)),
        Value::Number(_) => Err(mlua::Error::runtime(
            "ASTRA_VN_UI_CONTROLLER_VALUE_NON_FINITE: number must be finite",
        )),
        Value::String(value) => Ok(UiValue::String(value.to_str()?.to_owned())),
        Value::Table(table) => {
            let sequence_len = table.raw_len();
            if sequence_len > 0 {
                let mut values = Vec::with_capacity(sequence_len);
                for index in 1..=sequence_len {
                    values.push(lua_value_to_ui_value(table.raw_get(index)?, depth + 1)?);
                }
                let pair_count = table.clone().pairs::<Value, Value>().count();
                if pair_count != sequence_len {
                    return Err(mlua::Error::runtime(
                        "ASTRA_VN_UI_CONTROLLER_VALUE_TABLE: arrays cannot contain sparse or named fields",
                    ));
                }
                Ok(UiValue::List(values))
            } else {
                let mut values = BTreeMap::new();
                for pair in table.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    let Value::String(key) = key else {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_UI_CONTROLLER_VALUE_KEY: object keys must be strings",
                        ));
                    };
                    let key = key.to_str()?.to_owned();
                    if values
                        .insert(key, lua_value_to_ui_value(value, depth + 1)?)
                        .is_some()
                    {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_UI_CONTROLLER_VALUE_DUPLICATE: duplicate object key",
                        ));
                    }
                }
                Ok(UiValue::Map(values))
            }
        }
        _ => Err(mlua::Error::runtime(
            "ASTRA_VN_UI_CONTROLLER_VALUE_TYPE: functions, threads and userdata are forbidden",
        )),
    }
}

fn validate_handler(handlers: &Table, name: &str, required: bool) -> mlua::Result<()> {
    match handlers.get::<Value>(name)? {
        Value::Function(_) => Ok(()),
        Value::Nil if !required => Ok(()),
        Value::Nil => Err(mlua::Error::runtime(format!(
            "ASTRA_VN_UI_CONTROLLER_HANDLER_MISSING: {name} is required"
        ))),
        _ => Err(mlua::Error::runtime(format!(
            "ASTRA_VN_UI_CONTROLLER_HANDLER_TYPE: {name} must be a function"
        ))),
    }
}

fn validate_effects(
    manifest: &VnUiControllerManifest,
    invocation: ControllerInvocation<'_>,
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
    if !matches!(invocation, ControllerInvocation::Action(_))
        && effects
            .iter()
            .any(|effect| matches!(effect, VnUiControllerEffect::Forward { .. }))
    {
        return Err(LuauUiControllerError::Effect(
            "on_open/on_update cannot forward product actions".into(),
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

#[cfg(feature = "luau-runtime")]
fn create_table(lua: &Lua) -> mlua::Result<Table> {
    lua.create_table()
}

#[cfg(feature = "portable-luau-runtime")]
fn create_table(lua: &Lua) -> mlua::Result<Table> {
    Ok(lua.create_table())
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
    fn tsuinosora_modern_save_controller_requires_overwrite_confirmation() {
        let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
        host.register_source(include_str!(
            "../../../../../../Examples/TsuiNoSora/ProjectTemplate/Controllers/tsui_ui.luau"
        ))
        .expect("register TsuiNoSora controllers");
        let slot = UiValue::Map(BTreeMap::from([
            ("slot_id".into(), UiValue::String("slot.01".into())),
            ("occupied".into(), UiValue::Bool(true)),
        ]));
        let model = UiValue::Map(BTreeMap::from([(
            "slots".into(),
            UiValue::List(vec![slot]),
        )]));

        let effects = host
            .invoke_action(
                "tsui.system.save.modern",
                "astra.vn.ui_model.save.v1",
                &model,
                &VnUiAction::RequestSave {
                    slot_id: "slot.01".into(),
                },
                &mut VnUiSessionState::default(),
            )
            .expect("invoke save controller");

        assert_eq!(
            effects,
            vec![VnUiControllerEffect::OpenModal {
                view_id: "ui.tsui.modern.save_confirm".into(),
                model: UiValue::Map(BTreeMap::from([(
                    "slot_id".into(),
                    UiValue::String("slot.01".into()),
                )])),
            }]
        );
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

    #[astra_headless_test::test]
    fn optional_lifecycle_handlers_are_noops() {
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
        return { astra.ui.effect.forward(action) }
    end,
})
"#,
        )
        .expect("register");
        let mut session = VnUiSessionState::default();
        let model = UiValue::Map(BTreeMap::new());
        assert!(host
            .invoke_open(
                "controller.test",
                "astra.vn.ui_model.test.v1",
                &model,
                &mut session,
            )
            .expect("open")
            .is_empty());
        assert!(host
            .invoke_update(
                "controller.test",
                "astra.vn.ui_model.test.v1",
                &model,
                &VnUiControllerUpdate {
                    fixed_time_ns: 100,
                    delta_ns: 16,
                    generation: 1,
                },
                &mut session,
            )
            .expect("update")
            .is_empty());
    }

    #[astra_headless_test::test]
    fn lifecycle_handlers_receive_fixed_update_and_commit_session_state() {
        let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
        host.register_source(
            r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "session",
}, {
    on_open = function(ctx, model)
        return { astra.ui.effect.set_session_state("opened", true) }
    end,
    on_update = function(ctx, model, update)
        if update.fixed_time_ns ~= 100 or update.delta_ns ~= 16 or update.generation ~= 7 then
            error("fixed update mismatch")
        end
        return { astra.ui.effect.set_session_state("updated", true) }
    end,
    on_action = function(ctx, model, action)
        return { astra.ui.effect.forward(action) }
    end,
})
"#,
        )
        .expect("register");
        let mut session = VnUiSessionState::default();
        let model = UiValue::Map(BTreeMap::new());
        host.invoke_open(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &model,
            &mut session,
        )
        .expect("open");
        host.invoke_update(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &model,
            &VnUiControllerUpdate {
                fixed_time_ns: 100,
                delta_ns: 16,
                generation: 7,
            },
            &mut session,
        )
        .expect("update");
        assert_eq!(session.values().get("opened"), Some(&UiValue::Bool(true)));
        assert_eq!(session.values().get("updated"), Some(&UiValue::Bool(true)));
    }

    #[astra_headless_test::test]
    fn non_function_optional_handler_is_rejected_during_registration() {
        let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
        let error = host
            .register_source(
                r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_open = "invalid",
    on_action = function(ctx, model, action) return {} end,
})
"#,
            )
            .expect_err("invalid handler must fail registration");
        assert!(error.to_string().contains("on_open must be a function"));
    }
}
