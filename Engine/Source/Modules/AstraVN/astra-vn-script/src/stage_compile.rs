use std::collections::{BTreeMap, BTreeSet};

use astra_core::Diagnostic;

use crate::{
    lower::ParsedLine, AspectRatio, AudioCue, ExtensionCommandDescriptor, ExtensionFieldKind,
    ExtensionPresentationCommand, ExtensionValue, FixedScalar, MovieLoopMode, StageBlendMode,
    StageClipPolicy, StageCommand, StageLayerKind, StagePlacement, StageViewport, TimelineCommand,
    TimelineSpec, VnAudioBus, VnAudioSync, VnError, VnMovieEndBehavior, VnTimelineJoinPolicy,
    VnTimelineKeyframe, VnTimelineTrack,
};

pub(crate) fn compile_stage_command(line: &ParsedLine) -> Result<StageCommand, VnError> {
    match line.keyword.as_str() {
        "stage" => {
            validate_attrs(line, &["viewport", "safe_area"], &["viewport", "safe_area"])?;
            Ok(StageCommand::Configure {
                viewport: parse_viewport(line, required(line, "viewport")?)?,
                safe_area: parse_aspect(line, required(line, "safe_area")?)?,
            })
        }
        "layer" => {
            validate_attrs(
                line,
                &["id", "kind", "z", "blend", "clip", "input"],
                &["id", "kind", "z", "blend"],
            )?;
            Ok(StageCommand::DeclareLayer {
                id: symbol(line, "id")?,
                kind: parse_layer_kind(line, required(line, "kind")?)?,
                z: parse_i32(line, "z")?,
                blend: parse_blend(line, required(line, "blend")?)?,
                clip: line
                    .attr("clip")
                    .map(|value| parse_clip(line, value))
                    .transpose()?,
                input: line.attr("input").map(str::to_string),
            })
        }
        "background" => {
            validate_attrs(
                line,
                &["asset", "layer", "preset", "duration"],
                &["asset", "layer"],
            )?;
            Ok(StageCommand::Background {
                asset: asset_uri(line, "asset")?,
                layer: symbol(line, "layer")?,
                preset: optional_symbol(line, "preset")?,
                duration_ms: optional_u32(line, "duration", 0)?,
            })
        }
        "show" => {
            validate_attrs(
                line,
                &["id", "asset", "pose", "layer", "at", "preset"],
                &["id", "asset", "layer"],
            )?;
            Ok(StageCommand::Show {
                id: symbol(line, "id")?,
                asset: asset_uri(line, "asset")?,
                pose: optional_symbol(line, "pose")?,
                layer: symbol(line, "layer")?,
                placement: match line.attr("at").unwrap_or("center") {
                    "left" => StagePlacement::Left,
                    "center" => StagePlacement::Center,
                    "right" => StagePlacement::Right,
                    value => return Err(invalid_value(line, "at", value, "left,center,right")),
                },
                preset: optional_symbol(line, "preset")?,
            })
        }
        "hide" => {
            validate_attrs(line, &["id", "preset", "duration"], &["id"])?;
            Ok(StageCommand::Hide {
                id: symbol(line, "id")?,
                preset: optional_symbol(line, "preset")?,
                duration_ms: optional_u32(line, "duration", 0)?,
            })
        }
        "move" => {
            validate_attrs(
                line,
                &["id", "x", "y", "duration", "preset"],
                &["id", "x", "y", "duration"],
            )?;
            Ok(StageCommand::Move {
                id: symbol(line, "id")?,
                x: fixed(line, "x")?,
                y: fixed(line, "y")?,
                duration_ms: parse_u32(line, "duration")?,
                preset: optional_symbol(line, "preset")?,
            })
        }
        "camera" => {
            validate_attrs(
                line,
                &["target", "x", "y", "zoom", "rotation", "duration", "preset"],
                &["target"],
            )?;
            let zoom = optional_fixed(line, "zoom", FixedScalar::ONE)?;
            if zoom.millionths <= 0 {
                return Err(invalid_value(
                    line,
                    "zoom",
                    line.attr("zoom").unwrap_or(""),
                    "greater than zero",
                ));
            }
            Ok(StageCommand::Camera {
                target: symbol(line, "target")?,
                x: optional_fixed(line, "x", FixedScalar::ZERO)?,
                y: optional_fixed(line, "y", FixedScalar::ZERO)?,
                zoom,
                rotation: optional_fixed(line, "rotation", FixedScalar::ZERO)?,
                duration_ms: optional_u32(line, "duration", 0)?,
                preset: optional_symbol(line, "preset")?,
            })
        }
        "movie" => compile_movie(line),
        "voice" => compile_audio(line, VnAudioBus::Voice),
        "bgm" => compile_audio(line, VnAudioBus::Bgm),
        "se" => compile_audio(line, VnAudioBus::Se),
        "transition" => {
            validate_attrs(line, &["preset", "duration"], &["preset", "duration"])?;
            Ok(StageCommand::Transition {
                preset: symbol(line, "preset")?,
                duration_ms: parse_u32(line, "duration")?,
            })
        }
        "shake" => {
            validate_attrs(
                line,
                &["target", "strength", "duration"],
                &["target", "strength", "duration"],
            )?;
            Ok(StageCommand::Shake {
                target: symbol(line, "target")?,
                strength: fixed(line, "strength")?,
                duration_ms: parse_u32(line, "duration")?,
            })
        }
        "timeline" => compile_timeline(line),
        "effect" => {
            validate_attrs(
                line,
                &["text", "lip_sync", "filter", "fallback", "budget_ms"],
                &["text", "filter", "fallback", "budget_ms"],
            )?;
            Ok(StageCommand::Effect {
                target: symbol(line, "text")?,
                lip_sync: optional_bool(line, "lip_sync", false)?,
                filter: symbol(line, "filter")?,
                fallback: symbol(line, "fallback")?,
                budget_us: milliseconds_to_microseconds(line, parse_u32(line, "budget_ms")?)?,
            })
        }
        _ => Err(VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_STANDARD_COMMAND_UNSUPPORTED",
                "standard presentation command has no typed contract",
            )
            .with_source(line.source_ref())
            .with_field("command", &line.keyword),
        )),
    }
}

pub(crate) fn compile_extension_command(
    line: &ParsedLine,
    descriptor: &ExtensionCommandDescriptor,
) -> Result<ExtensionPresentationCommand, VnError> {
    if descriptor.command != line.keyword
        || !safe_symbol(&descriptor.provider_id)
        || !safe_schema(&descriptor.schema)
    {
        return Err(VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_DESCRIPTOR",
                "extension command descriptor is invalid or does not match the command",
            )
            .with_source(line.source_ref())
            .with_field("command", &line.keyword),
        ));
    }
    let allowed = descriptor.fields.keys().cloned().collect::<BTreeSet<_>>();
    for key in line.attrs.keys() {
        if !allowed.contains(key) {
            return Err(unknown_attr(line, key));
        }
    }
    let mut fields = BTreeMap::new();
    for (name, contract) in &descriptor.fields {
        let Some(value) = line.attr(name) else {
            if contract.required {
                return Err(missing_attr(line, name));
            }
            continue;
        };
        let value = match contract.kind {
            ExtensionFieldKind::String => ExtensionValue::String(value.to_string()),
            ExtensionFieldKind::Integer => ExtensionValue::Integer(
                value
                    .parse()
                    .map_err(|_| invalid_value(line, name, value, "signed integer"))?,
            ),
            ExtensionFieldKind::Fixed => ExtensionValue::Fixed(parse_fixed(line, name, value)?),
            ExtensionFieldKind::Boolean => ExtensionValue::Boolean(parse_bool(line, name, value)?),
            ExtensionFieldKind::Symbol => {
                validate_symbol(line, name, value)?;
                ExtensionValue::Symbol(value.to_string())
            }
            ExtensionFieldKind::AssetUri => {
                validate_asset_uri(line, name, value)?;
                ExtensionValue::AssetUri(value.to_string())
            }
        };
        fields.insert(name.clone(), value);
    }
    Ok(ExtensionPresentationCommand {
        command: descriptor.command.clone(),
        provider_id: descriptor.provider_id.clone(),
        schema: descriptor.schema.clone(),
        fields,
    })
}

fn compile_movie(line: &ParsedLine) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &[
            "layer", "asset", "alpha", "loop", "end", "fence", "fallback",
        ],
        &["layer", "asset"],
    )?;
    let alpha = optional_fixed(line, "alpha", FixedScalar::ONE)?;
    if !(0..=1_000_000).contains(&alpha.millionths) {
        return Err(invalid_value(
            line,
            "alpha",
            line.attr("alpha").unwrap_or(""),
            "0..1",
        ));
    }
    let end = match line.attr("end").unwrap_or("continue") {
        "continue" => VnMovieEndBehavior::Continue,
        "wait" => VnMovieEndBehavior::Wait,
        "hold" => VnMovieEndBehavior::Hold,
        value => return Err(invalid_value(line, "end", value, "continue,wait,hold")),
    };
    let fence = optional_symbol(line, "fence")?;
    let fallback = line
        .attr("fallback")
        .map(|value| {
            if value.starts_with("asset:/") {
                validate_asset_uri(line, "fallback", value)
            } else {
                validate_symbol(line, "fallback", value)
            }?;
            Ok::<String, VnError>(value.to_string())
        })
        .transpose()?;
    if end == VnMovieEndBehavior::Wait && (fence.is_none() || fallback.is_none()) {
        return Err(VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_MOVIE_WAIT_CONTRACT",
                "movie end:wait requires both fence and fallback",
            )
            .with_source(line.source_ref()),
        ));
    }
    Ok(StageCommand::Movie {
        layer: symbol(line, "layer")?,
        asset: asset_uri(line, "asset")?,
        alpha,
        loop_mode: if optional_bool(line, "loop", false)? {
            MovieLoopMode::Loop
        } else {
            MovieLoopMode::Once
        },
        end,
        fence,
        fallback,
    })
}

fn compile_audio(line: &ParsedLine, bus: VnAudioBus) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &["id", "asset", "loop", "fade", "sync", "fence", "bus"],
        &["asset"],
    )?;
    let expected_bus = match bus {
        VnAudioBus::Voice => "voice",
        VnAudioBus::Bgm => "bgm",
        VnAudioBus::Se => "se",
        VnAudioBus::Movie => "movie",
    };
    if let Some(declared_bus) = line.attr("bus") {
        if declared_bus != expected_bus {
            return Err(invalid_value(line, "bus", declared_bus, expected_bus));
        }
    }
    let sync = match line.attr("sync").unwrap_or("none") {
        "none" => VnAudioSync::None,
        "text" => VnAudioSync::Text,
        "fence" => VnAudioSync::Fence(symbol(line, "fence")?),
        value => return Err(invalid_value(line, "sync", value, "none,text,fence")),
    };
    let id = line
        .attr("id")
        .map(|value| {
            validate_symbol(line, "id", value)?;
            Ok::<String, VnError>(value.to_string())
        })
        .transpose()?
        .unwrap_or_else(|| line.stable_id());
    Ok(StageCommand::Audio(AudioCue {
        id,
        bus,
        asset: asset_uri(line, "asset")?,
        looped: optional_bool(line, "loop", false)?,
        fade_ms: optional_u32(line, "fade", 0)?,
        sync,
    }))
}

fn compile_timeline(line: &ParsedLine) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &[
            "id",
            "action",
            "reason",
            "target",
            "property",
            "keyframes",
            "join",
            "fence",
            "fallback",
            "budget_ms",
        ],
        &["id"],
    )?;
    let id = symbol(line, "id")?;
    match line.attr("action").unwrap_or("start") {
        "cancel" => Ok(StageCommand::Timeline(TimelineCommand::Cancel {
            id,
            reason: line.attr("reason").unwrap_or("requested").to_string(),
        })),
        "start" => {
            for name in ["target", "property", "keyframes", "budget_ms"] {
                if line.attr(name).is_none() {
                    return Err(missing_attr(line, name));
                }
            }
            let join = match line.attr("join").unwrap_or("fire_and_forget") {
                "fire_and_forget" => VnTimelineJoinPolicy::FireAndForget,
                "wait" | "block" => VnTimelineJoinPolicy::Block,
                "replace" | "replace_target" => VnTimelineJoinPolicy::ReplaceTarget,
                value => {
                    return Err(invalid_value(
                        line,
                        "join",
                        value,
                        "fire_and_forget,block,replace_target",
                    ))
                }
            };
            let fence = optional_symbol(line, "fence")?;
            if join == VnTimelineJoinPolicy::Block && fence.is_none() {
                return Err(missing_attr(line, "fence"));
            }
            Ok(StageCommand::Timeline(TimelineCommand::Start(
                TimelineSpec {
                    id,
                    join,
                    tracks: vec![VnTimelineTrack {
                        target: symbol(line, "target")?,
                        property: symbol(line, "property")?,
                        keyframes: parse_keyframes(line, required(line, "keyframes")?)?,
                    }],
                    fence,
                    fallback: optional_symbol(line, "fallback")?,
                    budget_us: milliseconds_to_microseconds(line, parse_u32(line, "budget_ms")?)?,
                },
            )))
        }
        value => Err(invalid_value(line, "action", value, "start,cancel")),
    }
}

fn parse_keyframes(line: &ParsedLine, value: &str) -> Result<Vec<VnTimelineKeyframe>, VnError> {
    let mut result = Vec::new();
    for pair in value.split(',') {
        let (time, scalar) = pair
            .split_once('=')
            .ok_or_else(|| invalid_value(line, "keyframes", value, "time=value list"))?;
        let time_ms = time
            .parse::<u32>()
            .map_err(|_| invalid_value(line, "keyframes", pair, "unsigned time=value"))?;
        if result
            .last()
            .is_some_and(|last: &VnTimelineKeyframe| last.time_ms >= time_ms)
        {
            return Err(invalid_value(
                line,
                "keyframes",
                pair,
                "strictly increasing times",
            ));
        }
        result.push(VnTimelineKeyframe {
            time_ms,
            value: parse_fixed(line, "keyframes", scalar)?,
        });
    }
    if result.len() < 2 {
        return Err(invalid_value(
            line,
            "keyframes",
            value,
            "at least two keyframes",
        ));
    }
    Ok(result)
}

fn validate_attrs(
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

fn required<'a>(line: &'a ParsedLine, key: &str) -> Result<&'a str, VnError> {
    line.attr(key).ok_or_else(|| missing_attr(line, key))
}

fn missing_attr(line: &ParsedLine, key: &str) -> VnError {
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

fn unknown_attr(line: &ParsedLine, key: &str) -> VnError {
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

fn invalid_value(line: &ParsedLine, key: &str, value: &str, expected: &str) -> VnError {
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

fn parse_viewport(line: &ParsedLine, value: &str) -> Result<StageViewport, VnError> {
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

fn parse_aspect(line: &ParsedLine, value: &str) -> Result<AspectRatio, VnError> {
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

fn parse_layer_kind(line: &ParsedLine, value: &str) -> Result<StageLayerKind, VnError> {
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

fn parse_blend(line: &ParsedLine, value: &str) -> Result<StageBlendMode, VnError> {
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

fn parse_clip(line: &ParsedLine, value: &str) -> Result<StageClipPolicy, VnError> {
    match value {
        "stage" => Ok(StageClipPolicy::Stage),
        "safe_area" => Ok(StageClipPolicy::SafeArea),
        _ => Err(invalid_value(line, "clip", value, "stage,safe_area")),
    }
}

fn symbol(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    let value = required(line, key)?;
    validate_symbol(line, key, value)?;
    Ok(value.to_string())
}

fn optional_symbol(line: &ParsedLine, key: &str) -> Result<Option<String>, VnError> {
    line.attr(key)
        .map(|value| {
            validate_symbol(line, key, value)?;
            Ok(value.to_string())
        })
        .transpose()
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

fn safe_schema(value: &str) -> bool {
    safe_symbol(value) && value.contains('.')
}

fn validate_symbol(line: &ParsedLine, key: &str, value: &str) -> Result<(), VnError> {
    if safe_symbol(value) {
        Ok(())
    } else {
        Err(invalid_value(line, key, value, "safe symbol"))
    }
}

fn asset_uri(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    let value = required(line, key)?;
    validate_asset_uri(line, key, value)?;
    Ok(value.to_string())
}

fn validate_asset_uri(line: &ParsedLine, key: &str, value: &str) -> Result<(), VnError> {
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

fn fixed(line: &ParsedLine, key: &str) -> Result<FixedScalar, VnError> {
    parse_fixed(line, key, required(line, key)?)
}
fn optional_fixed(
    line: &ParsedLine,
    key: &str,
    default: FixedScalar,
) -> Result<FixedScalar, VnError> {
    line.attr(key)
        .map(|value| parse_fixed(line, key, value))
        .unwrap_or(Ok(default))
}

fn parse_fixed(line: &ParsedLine, key: &str, value: &str) -> Result<FixedScalar, VnError> {
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

fn parse_u32(line: &ParsedLine, key: &str) -> Result<u32, VnError> {
    let value = required(line, key)?;
    value
        .parse::<u32>()
        .map_err(|_| invalid_value(line, key, value, "unsigned integer"))
}
fn optional_u32(line: &ParsedLine, key: &str, default: u32) -> Result<u32, VnError> {
    line.attr(key)
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| invalid_value(line, key, value, "unsigned integer"))
        })
        .unwrap_or(Ok(default))
}
fn parse_i32(line: &ParsedLine, key: &str) -> Result<i32, VnError> {
    let value = required(line, key)?;
    value
        .parse::<i32>()
        .map_err(|_| invalid_value(line, key, value, "signed integer"))
}
fn optional_bool(line: &ParsedLine, key: &str, default: bool) -> Result<bool, VnError> {
    line.attr(key)
        .map(|value| parse_bool(line, key, value))
        .unwrap_or(Ok(default))
}
fn parse_bool(line: &ParsedLine, key: &str, value: &str) -> Result<bool, VnError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_value(line, key, value, "true,false")),
    }
}
fn milliseconds_to_microseconds(line: &ParsedLine, value: u32) -> Result<u32, VnError> {
    value.checked_mul(1_000).ok_or_else(|| {
        invalid_value(
            line,
            "budget_ms",
            &value.to_string(),
            "bounded milliseconds",
        )
    })
}
