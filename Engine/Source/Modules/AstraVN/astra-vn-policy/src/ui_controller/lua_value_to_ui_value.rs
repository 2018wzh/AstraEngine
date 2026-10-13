use super::*;

pub(super) fn lua_value_to_ui_value(value: Value, depth: usize) -> mlua::Result<UiValue> {
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

pub(super) fn validate_handler(handlers: &Table, name: &str, required: bool) -> mlua::Result<bool> {
    match handlers.get::<Value>(name)? {
        Value::Function(_) => Ok(true),
        Value::Nil if !required => Ok(false),
        Value::Nil => Err(mlua::Error::runtime(format!(
            "ASTRA_VN_UI_CONTROLLER_HANDLER_MISSING: {name} is required"
        ))),
        _ => Err(mlua::Error::runtime(format!(
            "ASTRA_VN_UI_CONTROLLER_HANDLER_TYPE: {name} must be a function"
        ))),
    }
}

pub(super) fn validate_effects(
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

pub(super) fn runtime_error(error: mlua::Error) -> LuauUiControllerError {
    LuauUiControllerError::Runtime(error.to_string())
}

#[cfg(feature = "luau-runtime")]
pub(super) fn create_table(lua: &Lua) -> mlua::Result<Table> {
    lua.create_table()
}

#[cfg(feature = "portable-luau-runtime")]
pub(super) fn create_table(lua: &Lua) -> mlua::Result<Table> {
    Ok(lua.create_table())
}
