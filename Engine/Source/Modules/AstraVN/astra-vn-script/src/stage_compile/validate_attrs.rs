use super::*;

pub(super) fn validate_attrs(
    line: &ParsedLine,
    allowed: &[&str],
    required_names: &[&str],
) -> Result<(), VnError> {
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    for key in line.attrs.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(unknown_attr(line, key));
        }
    }
    for key in required_names {
        if line.attr(key).is_none() {
            return Err(missing_attr(line, key));
        }
    }
    Ok(())
}

pub(super) fn required<'a>(line: &'a ParsedLine, key: &str) -> Result<&'a str, VnError> {
    line.attr(key).ok_or_else(|| missing_attr(line, key))
}

pub(super) fn missing_attr(line: &ParsedLine, key: &str) -> VnError {
    VnError::Diagnostic(
        Diagnostic::blocking(
            "ASTRA_VN_STAGE_ATTRIBUTE_MISSING",
            "typed presentation command is missing a required attribute",
        )
        .with_source(line.source_ref())
        .with_field("command", &line.keyword)
        .with_field("attribute", key),
    )
}

pub(super) fn unknown_attr(line: &ParsedLine, key: &str) -> VnError {
    VnError::Diagnostic(
        Diagnostic::blocking(
            "ASTRA_VN_STAGE_ATTRIBUTE_UNKNOWN",
            "typed presentation command contains an undeclared attribute",
        )
        .with_source(line.source_ref())
        .with_field("command", &line.keyword)
        .with_field("attribute", key),
    )
}

pub(super) fn invalid_value(line: &ParsedLine, key: &str, value: &str, expected: &str) -> VnError {
    VnError::Diagnostic(
        Diagnostic::blocking(
            "ASTRA_VN_STAGE_ATTRIBUTE_INVALID",
            "typed presentation command attribute is invalid",
        )
        .with_source(line.source_ref())
        .with_field("command", &line.keyword)
        .with_field("attribute", key)
        .with_field("value", value)
        .with_field("expected", expected),
    )
}

pub(super) fn parse_viewport(line: &ParsedLine, value: &str) -> Result<StageViewport, VnError> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| invalid_value(line, "viewport", value, "WIDTHxHEIGHT"))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| invalid_value(line, "viewport", value, "positive WIDTHxHEIGHT"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| invalid_value(line, "viewport", value, "positive WIDTHxHEIGHT"))?;
    if width == 0 || height == 0 {
        return Err(invalid_value(
            line,
            "viewport",
            value,
            "positive WIDTHxHEIGHT",
        ));
    }
    Ok(StageViewport { width, height })
}

pub(super) fn parse_aspect(line: &ParsedLine, value: &str) -> Result<AspectRatio, VnError> {
    let (width, height) = value
        .split_once(':')
        .ok_or_else(|| invalid_value(line, "safe_area", value, "WIDTH:HEIGHT"))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| invalid_value(line, "safe_area", value, "positive WIDTH:HEIGHT"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| invalid_value(line, "safe_area", value, "positive WIDTH:HEIGHT"))?;
    if width == 0 || height == 0 {
        return Err(invalid_value(
            line,
            "safe_area",
            value,
            "positive WIDTH:HEIGHT",
        ));
    }
    Ok(AspectRatio { width, height })
}

pub(super) fn parse_rgba(line: &ParsedLine, value: &str) -> Result<[u8; 4], VnError> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_value(line, "color", value, "RRGGBBAA"));
    }
    let mut color = [0_u8; 4];
    for (index, component) in color.iter_mut().enumerate() {
        let start = index * 2;
        *component = u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|_| invalid_value(line, "color", value, "RRGGBBAA"))?;
    }
    Ok(color)
}

pub(super) fn parse_layer_kind(line: &ParsedLine, value: &str) -> Result<StageLayerKind, VnError> {
    match value {
        "background" => Ok(StageLayerKind::Background),
        "sprite" => Ok(StageLayerKind::Sprite),
        "video" => Ok(StageLayerKind::Video),
        "text" => Ok(StageLayerKind::Text),
        "cg" => Ok(StageLayerKind::Cg),
        "ui" => Ok(StageLayerKind::Ui),
        "effect" => Ok(StageLayerKind::Effect),
        _ => Err(invalid_value(
            line,
            "kind",
            value,
            "background,sprite,video,text,cg,ui,effect",
        )),
    }
}

pub(super) fn parse_blend(line: &ParsedLine, value: &str) -> Result<StageBlendMode, VnError> {
    match value {
        "normal" => Ok(StageBlendMode::Normal),
        "add" => Ok(StageBlendMode::Add),
        "multiply" => Ok(StageBlendMode::Multiply),
        "screen" => Ok(StageBlendMode::Screen),
        _ => Err(invalid_value(
            line,
            "blend",
            value,
            "normal,add,multiply,screen",
        )),
    }
}

pub(super) fn parse_clip(line: &ParsedLine, value: &str) -> Result<StageClipPolicy, VnError> {
    match value {
        "stage" => Ok(StageClipPolicy::Stage),
        "safe_area" => Ok(StageClipPolicy::SafeArea),
        _ => Err(invalid_value(line, "clip", value, "stage,safe_area")),
    }
}

pub(super) fn symbol(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    let value = required(line, key)?;
    validate_symbol(line, key, value)?;
    Ok(value.to_string())
}

pub(super) fn optional_symbol(line: &ParsedLine, key: &str) -> Result<Option<String>, VnError> {
    line.attr(key)
        .map(|value| {
            validate_symbol(line, key, value)?;
            Ok(value.to_string())
        })
        .transpose()
}

pub(super) fn safe_schema(value: &str) -> bool {
    safe_symbol(value) && value.contains('.')
}

pub(super) fn validate_symbol(line: &ParsedLine, key: &str, value: &str) -> Result<(), VnError> {
    if safe_symbol(value) {
        Ok(())
    } else {
        Err(invalid_value(line, key, value, "safe symbol"))
    }
}

pub(super) fn asset_uri(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    let value = required(line, key)?;
    validate_asset_uri(line, key, value)?;
    Ok(value.to_string())
}

pub(super) fn validate_asset_uri(line: &ParsedLine, key: &str, value: &str) -> Result<(), VnError> {
    let Some(path) = value.strip_prefix("asset:/") else {
        return Err(invalid_value(line, key, value, "asset:/ URI"));
    };
    if path.is_empty()
        || path.starts_with('/')
        || path.contains("..")
        || path.contains('\\')
        || !safe_symbol(path)
    {
        return Err(invalid_value(line, key, value, "normalized asset:/ URI"));
    }
    Ok(())
}

pub(super) fn fixed(line: &ParsedLine, key: &str) -> Result<FixedScalar, VnError> {
    parse_fixed(line, key, required(line, key)?)
}

pub(super) fn optional_fixed(
    line: &ParsedLine,
    key: &str,
    default: FixedScalar,
) -> Result<FixedScalar, VnError> {
    line.attr(key)
        .map(|value| parse_fixed(line, key, value))
        .unwrap_or(Ok(default))
}

pub(super) fn opacity(line: &ParsedLine, key: &str) -> Result<FixedScalar, VnError> {
    let value = optional_fixed(line, key, FixedScalar::ONE)?;
    if !(0..=1_000_000).contains(&value.millionths) {
        return Err(invalid_value(
            line,
            key,
            line.attr(key).unwrap_or(""),
            "0..1",
        ));
    }
    Ok(value)
}

pub(super) fn parse_fixed(
    line: &ParsedLine,
    key: &str,
    value: &str,
) -> Result<FixedScalar, VnError> {
    let negative = value.starts_with('-');
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
    {
        return Err(invalid_value(
            line,
            key,
            value,
            "decimal with at most 6 fractional digits",
        ));
    }
    let whole = whole
        .parse::<i64>()
        .map_err(|_| invalid_value(line, key, value, "bounded decimal"))?;
    let mut fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<i64>()
            .map_err(|_| invalid_value(line, key, value, "bounded decimal"))?
    };
    for _ in fraction.len()..6 {
        fraction_value *= 10;
    }
    let magnitude = whole
        .checked_mul(1_000_000)
        .and_then(|whole| whole.checked_add(fraction_value))
        .ok_or_else(|| invalid_value(line, key, value, "bounded decimal"))?;
    Ok(FixedScalar {
        millionths: if negative { -magnitude } else { magnitude },
    })
}

pub(super) fn parse_u32(line: &ParsedLine, key: &str) -> Result<u32, VnError> {
    let value = required(line, key)?;
    value
        .parse::<u32>()
        .map_err(|_| invalid_value(line, key, value, "unsigned integer"))
}

pub(super) fn optional_u32(line: &ParsedLine, key: &str, default: u32) -> Result<u32, VnError> {
    line.attr(key)
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| invalid_value(line, key, value, "unsigned integer"))
        })
        .unwrap_or(Ok(default))
}

pub(super) fn parse_i32(line: &ParsedLine, key: &str) -> Result<i32, VnError> {
    let value = required(line, key)?;
    value
        .parse::<i32>()
        .map_err(|_| invalid_value(line, key, value, "signed integer"))
}

pub(super) fn optional_bool(line: &ParsedLine, key: &str, default: bool) -> Result<bool, VnError> {
    line.attr(key)
        .map(|value| parse_bool(line, key, value))
        .unwrap_or(Ok(default))
}

pub(super) fn parse_bool(line: &ParsedLine, key: &str, value: &str) -> Result<bool, VnError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_value(line, key, value, "true,false")),
    }
}

pub(super) fn parse_interrupt(line: &ParsedLine) -> Result<PresentationInterruptPolicy, VnError> {
    match required(line, "interrupt")? {
        "queue" => Ok(PresentationInterruptPolicy::Queue),
        "replace_from_current" => Ok(PresentationInterruptPolicy::ReplaceFromCurrent),
        "snap_then_start" => Ok(PresentationInterruptPolicy::SnapThenStart),
        "reject" => Ok(PresentationInterruptPolicy::Reject),
        value => Err(invalid_value(
            line,
            "interrupt",
            value,
            "queue,replace_from_current,snap_then_start,reject",
        )),
    }
}

pub(super) fn milliseconds_to_microseconds(line: &ParsedLine, value: u32) -> Result<u32, VnError> {
    value.checked_mul(1_000).ok_or_else(|| {
        invalid_value(
            line,
            "budget_ms",
            &value.to_string(),
            "bounded milliseconds",
        )
    })
}
