use super::family_loader::text;
use crate::{
    FamilyCapability, FamilyPluginDescriptor, INDEPENDENT_FAMILY_ABI_FINGERPRINT, model::*,
};
use astra_emu_abi as abi;

pub(super) unsafe fn descriptor(
    raw: abi::Descriptor,
) -> FamilyResult<(FamilyPluginDescriptor, Vec<ConfigField>)> {
    let invalid = || FamilyError::invalid("ASTRA_EMU_CORE_DESCRIPTOR", "invalid core descriptor");
    if !raw.header.accepts::<abi::Descriptor>()
        || raw.capabilities & (abi::CAP_CPU_SURFACE | abi::CAP_GPU_SURFACE) == 0
    {
        return Err(invalid());
    }
    let formats = unsafe { abi::borrowed_array(raw.formats, raw.format_count, 128) }
        .map_err(|_| invalid())?;
    let fields = unsafe {
        abi::borrowed_array(
            raw.configuration,
            raw.configuration_count,
            abi::MAX_CONFIG_FIELDS,
        )
    }
    .map_err(|_| invalid())?;
    unsafe { abi::validate_config(fields, &[]) }.map_err(|_| invalid())?;
    let mut configuration = Vec::with_capacity(fields.len());
    for field in fields {
        let default = unsafe { value(field.default_value) }?;
        let kind = match field.default_value.kind {
            abi::VALUE_BOOL => ConfigKind::Bool,
            abi::VALUE_INTEGER => ConfigKind::Integer {
                min: field.integer_min,
                max: field.integer_max,
            },
            abi::VALUE_NUMBER => ConfigKind::Number {
                min: field.number_min,
                max: field.number_max,
            },
            abi::VALUE_STRING => ConfigKind::String {
                max_bytes: field.max_bytes,
            },
            abi::VALUE_ENUM => {
                let choices = unsafe {
                    abi::borrowed_array(field.choices, field.choice_count, abi::MAX_CONFIG_FIELDS)
                }
                .map_err(|_| invalid())?;
                ConfigKind::Enum {
                    choices: choices
                        .iter()
                        .map(|&v| unsafe { text(v, 128) })
                        .collect::<FamilyResult<_>>()?,
                }
            }
            _ => return Err(invalid()),
        };
        configuration.push(ConfigField {
            id: unsafe { text(field.id, 128) }?,
            label: unsafe { text(field.label, 256) }?,
            group: unsafe { text(field.group, 256) }?,
            kind,
            default,
        });
    }
    let mut capabilities = Vec::new();
    if raw.capabilities & abi::CAP_CPU_SURFACE != 0 {
        capabilities.push(FamilyCapability::CpuFrame);
    }
    if raw.capabilities & abi::CAP_GPU_SURFACE != 0 {
        capabilities.push(FamilyCapability::GpuFrame);
    }
    if raw.capabilities & abi::CAP_TEXT_HOOK != 0 {
        capabilities.push(FamilyCapability::TextReplacement);
    }
    let descriptor = FamilyPluginDescriptor {
        family_id: unsafe { text(raw.family_id, 256) }?,
        plugin_id: unsafe { text(raw.plugin_id, 256) }?,
        version: unsafe { text(raw.version, 256) }?,
        abi_fingerprint: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        capabilities,
        supported_formats: formats
            .iter()
            .map(|&v| unsafe { text(v, 256) })
            .collect::<FamilyResult<_>>()?,
    };
    descriptor.validate().map_err(|_| invalid())?;
    validate_config_schema(&configuration)?;
    Ok((descriptor, configuration))
}
unsafe fn value(value: abi::ConfigValue) -> FamilyResult<ConfigValue> {
    Ok(match value.kind {
        abi::VALUE_BOOL => ConfigValue::Bool(value.integer != 0),
        abi::VALUE_INTEGER => ConfigValue::Integer(value.integer),
        abi::VALUE_NUMBER => ConfigValue::Number(value.number),
        abi::VALUE_STRING => {
            ConfigValue::String(unsafe { text(value.text, abi::MAX_CONFIG_TEXT) }?)
        }
        abi::VALUE_ENUM => ConfigValue::Enum(unsafe { text(value.text, 128) }?),
        _ => {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_CORE_CONFIG",
                "unknown configuration type",
            ));
        }
    })
}
pub(super) fn entries(entries: &[ConfigEntry]) -> Vec<abi::ConfigEntry> {
    entries
        .iter()
        .map(|entry| {
            let mut value = abi::ConfigValue {
                kind: 0,
                reserved: 0,
                integer: 0,
                number: 0.0,
                text: abi::Bytes::EMPTY,
            };
            match &entry.value {
                ConfigValue::Bool(v) => {
                    value.kind = abi::VALUE_BOOL;
                    value.integer = i64::from(*v);
                }
                ConfigValue::Integer(v) => {
                    value.kind = abi::VALUE_INTEGER;
                    value.integer = *v;
                }
                ConfigValue::Number(v) => {
                    value.kind = abi::VALUE_NUMBER;
                    value.number = *v;
                }
                ConfigValue::String(v) => {
                    value.kind = abi::VALUE_STRING;
                    value.text = abi::Bytes::new(v.as_bytes());
                }
                ConfigValue::Enum(v) => {
                    value.kind = abi::VALUE_ENUM;
                    value.text = abi::Bytes::new(v.as_bytes());
                }
            }
            abi::ConfigEntry {
                id: abi::Bytes::new(entry.id.as_bytes()),
                value,
            }
        })
        .collect()
}
