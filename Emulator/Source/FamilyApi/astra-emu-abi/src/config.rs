use crate::{borrowed_array, Bytes, Status, INVALID_ARGUMENT};

pub const MAX_CONFIG_FIELDS: usize = 128;
pub const MAX_CONFIG_TEXT: usize = 4096;
pub const VALUE_BOOL: u32 = 1;
pub const VALUE_INTEGER: u32 = 2;
pub const VALUE_NUMBER: u32 = 3;
pub const VALUE_STRING: u32 = 4;
pub const VALUE_ENUM: u32 = 5;

/// Only the member selected by `kind` is meaningful. Boolean uses integer 0/1.
/// Reserved members must be zero; no Rust enum or owning string crosses the ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConfigValue {
    pub kind: u32,
    pub reserved: u32,
    pub integer: i64,
    pub number: f64,
    pub text: Bytes,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConfigEntry {
    pub id: Bytes,
    pub value: ConfigValue,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConfigField {
    pub id: Bytes,
    pub label: Bytes,
    pub group: Bytes,
    pub default_value: ConfigValue,
    pub integer_min: i64,
    pub integer_max: i64,
    pub number_min: f64,
    pub number_max: f64,
    pub max_bytes: u32,
    pub reserved: u32,
    pub choices: *const Bytes,
    pub choice_count: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub index: usize,
    pub reason: &'static str,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
impl ConfigField {
    /// # Safety
    /// All spans in this field and `value` must be valid during the call.
    pub unsafe fn accepts(&self, value: &ConfigValue) -> Result<bool, Status> {
        if value.kind != self.default_value.kind || value.reserved != 0 {
            return Ok(false);
        }
        Ok(match value.kind {
            VALUE_BOOL => matches!(value.integer, 0 | 1),
            VALUE_INTEGER => (self.integer_min..=self.integer_max).contains(&value.integer),
            VALUE_NUMBER => {
                value.number.is_finite()
                    && (self.number_min..=self.number_max).contains(&value.number)
            }
            VALUE_STRING => {
                unsafe { value.text.text(self.max_bytes as usize)? }.len()
                    <= self.max_bytes as usize
            }
            VALUE_ENUM => {
                let text = unsafe { value.text.text(128)? };
                let choices =
                    unsafe { borrowed_array(self.choices, self.choice_count, MAX_CONFIG_FIELDS)? };
                let mut found = false;
                for choice in choices {
                    found |= unsafe { choice.text(128)? } == text;
                }
                found
            }
            _ => false,
        })
    }
}

/// Validate schema and explicit values without allocating or materializing a
/// second copy. A core reads omitted values from the matching field's default.
/// # Safety
/// All nested pointers must remain readable and immutable throughout the call.
pub unsafe fn validate_config(
    fields: &[ConfigField],
    entries: &[ConfigEntry],
) -> Result<(), ConfigError> {
    let fail = |index, reason| ConfigError { index, reason };
    if fields.len() > MAX_CONFIG_FIELDS || entries.len() > MAX_CONFIG_FIELDS {
        return Err(fail(0, "too many configuration fields"));
    }
    for (index, field) in fields.iter().enumerate() {
        let validate = || -> Result<(), Status> {
            let id = unsafe { field.id.text(128)? };
            if !valid_id(id) || field.reserved != 0 {
                return Err(INVALID_ARGUMENT);
            }
            for previous in &fields[..index] {
                if unsafe { previous.id.text(128)? } == id {
                    return Err(INVALID_ARGUMENT);
                }
            }
            let label = unsafe { field.label.text(256)? };
            let group = unsafe { field.group.text(256)? };
            if label.is_empty()
                || label.chars().any(char::is_control)
                || group.chars().any(char::is_control)
            {
                return Err(INVALID_ARGUMENT);
            }
            let range_valid = match field.default_value.kind {
                VALUE_BOOL => true,
                VALUE_INTEGER => field.integer_min <= field.integer_max,
                VALUE_NUMBER => {
                    field.number_min.is_finite()
                        && field.number_max.is_finite()
                        && field.number_min <= field.number_max
                }
                VALUE_STRING => field.max_bytes as usize <= MAX_CONFIG_TEXT,
                VALUE_ENUM => {
                    let choices = unsafe {
                        borrowed_array(field.choices, field.choice_count, MAX_CONFIG_FIELDS)?
                    };
                    if choices.is_empty() {
                        return Err(INVALID_ARGUMENT);
                    }
                    for (i, choice) in choices.iter().enumerate() {
                        let text = unsafe { choice.text(128)? };
                        if !valid_id(text) {
                            return Err(INVALID_ARGUMENT);
                        }
                        for prior in &choices[..i] {
                            if unsafe { prior.text(128)? } == text {
                                return Err(INVALID_ARGUMENT);
                            }
                        }
                    }
                    true
                }
                _ => false,
            };
            if !range_valid || !unsafe { field.accepts(&field.default_value)? } {
                return Err(INVALID_ARGUMENT);
            }
            Ok(())
        };
        validate().map_err(|_| fail(index, "invalid schema, duplicate ID or invalid default"))?;
    }
    for (index, entry) in entries.iter().enumerate() {
        let id = unsafe { entry.id.text(128) }.map_err(|_| fail(index, "invalid value ID"))?;
        for previous in &entries[..index] {
            if unsafe { previous.id.text(128) }.map_err(|_| fail(index, "invalid value ID"))? == id
            {
                return Err(fail(index, "duplicate value ID"));
            }
        }
        let field = fields
            .iter()
            .find(|field| unsafe { field.id.text(128) } == Ok(id))
            .ok_or_else(|| fail(index, "unknown value ID"))?;
        if !unsafe { field.accepts(&entry.value) }
            .map_err(|_| fail(index, "invalid value encoding"))?
        {
            return Err(fail(index, "value type or range mismatch"));
        }
    }
    Ok(())
}
