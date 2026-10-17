use super::*;
use astra_emu_family_api as legacy;
use std::sync::LazyLock;
struct Schema {
    descriptor: abi::Descriptor,
    _source: legacy::FamilyDescriptor,
    _fields: Vec<abi::ConfigField>,
    _formats: Vec<abi::Bytes>,
    _choices: Vec<Vec<abi::Bytes>>,
}
// All pointers refer to immutable module-lifetime allocations built once.
unsafe impl Send for Schema {}
unsafe impl Sync for Schema {}
static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
        let source = crate::provider::musica_descriptor();
        let mut all_choices = Vec::new();
        let mut fields = Vec::with_capacity(source.configuration.len());
        for field in &source.configuration {
            let mut target = abi::ConfigField { id: bytes(&field.id), label: bytes(&field.label), group: bytes(&field.group),
                default_value: value(&field.default), integer_min: 0, integer_max: 0, number_min: 0.0, number_max: 0.0,
                max_bytes: 0, reserved: 0, choices: std::ptr::null(), choice_count: 0 };
            match &field.kind {
                legacy::ConfigKind::Bool => {},
                legacy::ConfigKind::Integer { min, max } => { target.integer_min = *min; target.integer_max = *max; },
                legacy::ConfigKind::Number { min, max } => { target.number_min = *min; target.number_max = *max; },
                legacy::ConfigKind::String { max_bytes } => target.max_bytes = *max_bytes,
                legacy::ConfigKind::Enum { choices } => {
                    let choices: Vec<_> = choices.iter().map(|v| bytes(v)).collect();
                    target.choices = choices.as_ptr(); target.choice_count = choices.len();
                    all_choices.push(choices);
                }
            }
            if matches!(field.id.as_str(), "logical_width" | "logical_height" | "render_width" | "render_height") {
                target.integer_max = target.integer_max.min(16384);
            }
            fields.push(target);
        }
        let formats: Vec<_> = source.supported_formats.iter().map(|v| bytes(v)).collect();
        Schema { descriptor: abi::Descriptor { header: abi::Header::of::<abi::Descriptor>(), family_id: bytes(&source.family_id),
            plugin_id: bytes(&source.plugin_id), version: bytes(&source.version), capabilities: abi::CAP_CPU_SURFACE | abi::CAP_TEXT_HOOK,
            formats: formats.as_ptr(), format_count: formats.len(), configuration: fields.as_ptr(), configuration_count: fields.len() },
            _source: source, _fields: fields, _formats: formats, _choices: all_choices }
});
pub(super) fn descriptor() -> abi::Descriptor { SCHEMA.descriptor }
fn bytes(value: &str) -> abi::Bytes { abi::Bytes::new(value.as_bytes()) }
fn value(value: &legacy::ConfigValue) -> abi::ConfigValue {
    let mut target = abi::ConfigValue { kind: 0, reserved: 0, integer: 0, number: 0.0, text: abi::Bytes::EMPTY };
    match value {
        legacy::ConfigValue::Bool(value) => { target.kind = abi::VALUE_BOOL; target.integer = i64::from(*value); },
        legacy::ConfigValue::Integer(value) => { target.kind = abi::VALUE_INTEGER; target.integer = *value; },
        legacy::ConfigValue::Number(value) => { target.kind = abi::VALUE_NUMBER; target.number = *value; },
        legacy::ConfigValue::String(value) => { target.kind = abi::VALUE_STRING; target.text = bytes(value); },
        legacy::ConfigValue::Enum(value) => { target.kind = abi::VALUE_ENUM; target.text = bytes(value); },
    }
    target
}
pub(super) unsafe fn entries(request: &abi::OpenRequest) -> Result<Vec<legacy::ConfigEntry>, Failure> {
    let descriptor = descriptor();
    let fields = unsafe { abi::borrowed_array(descriptor.configuration, descriptor.configuration_count, abi::MAX_CONFIG_FIELDS) }.map_err(invalid)?;
    let entries = unsafe { abi::borrowed_array(request.configuration, request.configuration_count, abi::MAX_CONFIG_FIELDS) }.map_err(invalid)?;
    unsafe { abi::validate_config(fields, entries) }.map_err(|cause| Failure::new(abi::INVALID_ARGUMENT,
        "ASTRA_EMU_MUSICA_CONFIG", &format!("configuration entry {}: {}", cause.index, cause.reason)))?;
    entries.iter().map(|entry| {
        let id = unsafe { entry.id.text(128) }.map_err(invalid)?;
        let value = match entry.value.kind {
            abi::VALUE_BOOL => legacy::ConfigValue::Bool(entry.value.integer != 0),
            abi::VALUE_INTEGER => legacy::ConfigValue::Integer(entry.value.integer),
            abi::VALUE_NUMBER => legacy::ConfigValue::Number(entry.value.number),
            abi::VALUE_STRING => legacy::ConfigValue::String(unsafe { entry.value.text.text(abi::MAX_CONFIG_TEXT) }.map_err(invalid)?.into()),
            abi::VALUE_ENUM => legacy::ConfigValue::Enum(unsafe { entry.value.text.text(128) }.map_err(invalid)?.into()),
            _ => return Err(invalid(abi::INVALID_ARGUMENT)),
        };
        Ok(legacy::ConfigEntry { id: id.into(), value })
    }).collect()
}
