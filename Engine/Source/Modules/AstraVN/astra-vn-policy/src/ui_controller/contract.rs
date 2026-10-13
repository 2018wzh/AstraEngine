use super::*;

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

pub(super) struct RegisteredController {
    pub(super) manifest: VnUiControllerManifest,
    pub(super) handlers: ControllerHandlerSet,
    /// Retained for source inspection and deterministic registration diagnostics.
    /// Invocation uses the native bytecode image where the runtime supports it.
    pub(super) source: Arc<str>,
    pub(super) program: ControllerProgram,
}

#[derive(Clone)]
pub(super) enum ControllerProgram {
    #[cfg(feature = "luau-runtime")]
    Bytecode(Arc<[u8]>),
    #[cfg(feature = "portable-luau-runtime")]
    Source(Arc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LoadedController {
    pub(super) manifest: VnUiControllerManifest,
    pub(super) handlers: ControllerHandlerSet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ControllerHandlerSet {
    pub(super) on_open: bool,
    pub(super) on_action: bool,
    pub(super) on_update: bool,
}

impl ControllerHandlerSet {
    pub(super) fn supports(self, invocation: ControllerInvocation<'_>) -> bool {
        match invocation {
            ControllerInvocation::Open => self.on_open,
            ControllerInvocation::Action(_) => self.on_action,
            ControllerInvocation::Update(_) => self.on_update,
        }
    }
}

/// Deterministic host for project-authored UI controllers.
///
/// Each callback receives a fresh capability sandbox. Session state is held by
/// Rust, so Lua functions, threads and userdata can never enter save/replay.
pub struct LuauUiControllerHost {
    pub(super) budget: PolicyExecutionBudget,
    pub(super) controllers: BTreeMap<String, RegisteredController>,
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
        let source: Arc<str> = Arc::from(source.into());
        let (_, registrations, program) = compile_controller_source(&source, self.budget)?;
        if registrations.is_empty() {
            return Err(LuauUiControllerError::Registration(
                "a controller source must register at least one controller".into(),
            ));
        }
        for loaded in registrations.values() {
            loaded
                .manifest
                .validate()
                .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
            if self.controllers.contains_key(&loaded.manifest.id) {
                return Err(LuauUiControllerError::Duplicate);
            }
        }
        self.controllers
            .extend(registrations.into_iter().map(|(id, loaded)| {
                (
                    id,
                    RegisteredController {
                        manifest: loaded.manifest,
                        handlers: loaded.handlers,
                        source: Arc::clone(&source),
                        program: program.clone(),
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
        self.controllers.get(id).map(|entry| entry.source.as_ref())
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

    pub(super) fn invoke(
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
        let (handler_name, required) = invocation.handler();
        if !registered.handlers.supports(invocation) {
            debug_assert!(!required, "required controller handler was not registered");
            return Ok(Vec::new());
        }
        let (lua, registrations) = load_controller_program(&registered.program, self.budget)?;
        let loaded = registrations.get(id).ok_or_else(|| {
            LuauUiControllerError::Registration(
                "controller source did not reproduce its registered identity".into(),
            )
        })?;
        if loaded.manifest != registered.manifest || loaded.handlers != registered.handlers {
            return Err(LuauUiControllerError::Registration(
                "controller manifest or handler set changed between registration and invocation"
                    .into(),
            ));
        }
        let registry: Table = lua
            .globals()
            .get("__astra_ui_controllers")
            .map_err(runtime_error)?;
        let handlers: Table = registry.get(id).map_err(runtime_error)?;
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
pub(super) enum ControllerInvocation<'a> {
    Open,
    Action(&'a VnUiAction),
    Update(&'a VnUiControllerUpdate),
}

impl ControllerInvocation<'_> {
    pub(super) fn handler(self) -> (&'static str, bool) {
        match self {
            Self::Open => ("on_open", false),
            Self::Action(_) => ("on_action", true),
            Self::Update(_) => ("on_update", false),
        }
    }
}

pub(super) fn compile_controller_source(
    source: &str,
    budget: PolicyExecutionBudget,
) -> Result<(Lua, BTreeMap<String, LoadedController>, ControllerProgram), LuauUiControllerError> {
    let lua = create_sandboxed_lua(budget)
        .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
    let registrations = Rc::new(RefCell::new(BTreeMap::new()));
    install_ui_api(&lua, Rc::clone(&registrations))?;
    let function = lua
        .load(source)
        .set_name("astra-ui-controller")
        .into_function()
        .map_err(runtime_error)?;
    #[cfg(feature = "luau-runtime")]
    let program = ControllerProgram::Bytecode(Arc::from(
        Compiler::new()
            .set_optimization_level(2)
            .set_debug_level(0)
            .compile(source)
            .map_err(runtime_error)?,
    ));
    #[cfg(feature = "portable-luau-runtime")]
    let program = ControllerProgram::Source(Arc::from(source));
    function.call::<()>(()).map_err(runtime_error)?;
    let registrations = registrations.borrow().clone();
    Ok((lua, registrations, program))
}

pub(super) fn load_controller_program(
    program: &ControllerProgram,
    budget: PolicyExecutionBudget,
) -> Result<(Lua, BTreeMap<String, LoadedController>), LuauUiControllerError> {
    let lua = create_sandboxed_lua(budget)
        .map_err(|error| LuauUiControllerError::Registration(error.to_string()))?;
    let registrations = Rc::new(RefCell::new(BTreeMap::new()));
    install_ui_api(&lua, Rc::clone(&registrations))?;
    match program {
        #[cfg(feature = "luau-runtime")]
        ControllerProgram::Bytecode(bytes) => lua
            .load(bytes.as_ref())
            .set_name("astra-ui-controller")
            .exec()
            .map_err(runtime_error)?,
        #[cfg(feature = "portable-luau-runtime")]
        ControllerProgram::Source(source) => lua
            .load(source.as_ref())
            .set_name("astra-ui-controller")
            .exec()
            .map_err(runtime_error)?,
    }
    let registrations = registrations.borrow().clone();
    Ok((lua, registrations))
}

pub(super) fn install_ui_api(
    lua: &Lua,
    registrations: Rc<RefCell<BTreeMap<String, LoadedController>>>,
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
                let handler_set = ControllerHandlerSet {
                    on_action: validate_handler(&handlers, "on_action", true)?,
                    on_open: validate_handler(&handlers, "on_open", false)?,
                    on_update: validate_handler(&handlers, "on_update", false)?,
                };
                if registrations
                    .borrow_mut()
                    .insert(
                        id.clone(),
                        LoadedController {
                            manifest,
                            handlers: handler_set,
                        },
                    )
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
