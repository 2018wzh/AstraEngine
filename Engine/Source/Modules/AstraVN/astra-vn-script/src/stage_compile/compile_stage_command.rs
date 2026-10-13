use super::*;

pub(crate) fn compile_stage_command(line: &ParsedLine) -> Result<StageCommand, VnError> {
    match line.keyword.as_str() {
        "preload" => {
            validate_attrs(line, &["asset"], &["asset"])?;
            Ok(StageCommand::Preload {
                asset: asset_uri(line, "asset")?,
            })
        }
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
                &["asset", "layer", "preset", "duration", "interrupt"],
                &["asset", "layer", "interrupt"],
            )?;
            Ok(StageCommand::Background {
                asset: asset_uri(line, "asset")?,
                layer: symbol(line, "layer")?,
                preset: optional_symbol(line, "preset")?,
                duration_ms: optional_u32(line, "duration", 0)?,
                interrupt: parse_interrupt(line)?,
            })
        }
        "show" => {
            validate_attrs(
                line,
                &[
                    "id",
                    "asset",
                    "pose",
                    "layer",
                    "at",
                    "fit",
                    "opacity",
                    "preset",
                    "interrupt",
                ],
                &["id", "asset", "layer", "interrupt"],
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
                fit: match line.attr("fit").unwrap_or("contain_height") {
                    "contain_height" => StageFitMode::ContainHeight,
                    "native" => StageFitMode::Native,
                    value => {
                        return Err(invalid_value(line, "fit", value, "contain_height,native"))
                    }
                },
                opacity: opacity(line, "opacity")?,
                preset: optional_symbol(line, "preset")?,
                interrupt: parse_interrupt(line)?,
            })
        }
        "hide" => {
            validate_attrs(
                line,
                &["id", "preset", "duration", "interrupt"],
                &["id", "interrupt"],
            )?;
            Ok(StageCommand::Hide {
                id: symbol(line, "id")?,
                preset: optional_symbol(line, "preset")?,
                duration_ms: optional_u32(line, "duration", 0)?,
                interrupt: parse_interrupt(line)?,
            })
        }
        "clear_layer" => {
            validate_attrs(
                line,
                &["layer", "duration", "interrupt"],
                &["layer", "interrupt"],
            )?;
            Ok(StageCommand::ClearLayer {
                layer: symbol(line, "layer")?,
                duration_ms: optional_u32(line, "duration", 0)?,
                interrupt: parse_interrupt(line)?,
            })
        }
        "layer_visibility" => {
            validate_attrs(line, &["layer", "visible"], &["layer", "visible"])?;
            Ok(StageCommand::SetLayerVisibility {
                layer: symbol(line, "layer")?,
                visible: parse_bool(line, "visible", required(line, "visible")?)?,
            })
        }
        "backdrop" => {
            validate_attrs(line, &["color"], &["color"])?;
            Ok(StageCommand::Backdrop {
                color: parse_rgba(line, required(line, "color")?)?,
            })
        }
        "shade" => {
            validate_attrs(line, &["color", "opacity"], &["opacity"])?;
            let color = match line.attrs.get("color") {
                Some(value) => parse_rgba(line, value)?,
                None => [0, 0, 0, 255],
            };
            if color[3] != 255 {
                return Err(invalid_value(
                    line,
                    "color",
                    line.attr("color").unwrap_or(""),
                    "an opaque #RRGGBB or #RRGGBBAA color",
                ));
            }
            Ok(StageCommand::Shade {
                color,
                opacity: opacity(line, "opacity")?,
            })
        }
        "skip_allowed" => {
            validate_attrs(line, &["allowed"], &["allowed"])?;
            Ok(StageCommand::SetSkipAllowed {
                allowed: parse_bool(line, "allowed", required(line, "allowed")?)?,
            })
        }
        "move" => {
            validate_attrs(
                line,
                &["id", "x", "y", "duration", "preset", "interrupt"],
                &["id", "x", "y", "duration", "interrupt"],
            )?;
            Ok(StageCommand::Move {
                id: symbol(line, "id")?,
                x: fixed(line, "x")?,
                y: fixed(line, "y")?,
                duration_ms: parse_u32(line, "duration")?,
                preset: optional_symbol(line, "preset")?,
                interrupt: parse_interrupt(line)?,
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
        "audio" => compile_audio_control(line),
        "transition" => {
            validate_attrs(
                line,
                &["preset", "duration", "descriptor"],
                &["preset", "duration"],
            )?;
            Ok(StageCommand::Transition {
                preset: symbol(line, "preset")?,
                duration_ms: parse_u32(line, "duration")?,
                descriptor_id: optional_symbol(line, "descriptor")?,
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
        "timeline" => timeline::compile_timeline(line),
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

pub(super) fn compile_audio_control(line: &ParsedLine) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &["action", "target", "duration", "fence"],
        &["action", "target"],
    )?;
    let action = match required(line, "action")? {
        "pause" => VnAudioControlAction::Pause,
        "resume" => VnAudioControlAction::Resume,
        "stop" => VnAudioControlAction::Stop,
        "fade_stop" => {
            let duration_ms = optional_u32(line, "duration", 0)?;
            if duration_ms == 0 {
                return Err(invalid_value(
                    line,
                    "duration",
                    "0",
                    "a positive millisecond duration",
                ));
            }
            VnAudioControlAction::FadeStop {
                duration_ms,
                fence: symbol(line, "fence")?,
            }
        }
        value => {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_AUDIO_CONTROL_ACTION",
                    "audio control action must be pause, resume, stop, or fade_stop",
                )
                .with_source(line.source_ref())
                .with_field("action", value),
            ));
        }
    };
    Ok(StageCommand::AudioControl(AudioControl {
        id: line.stable_id(),
        action,
        target: symbol(line, "target")?,
    }))
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

pub(super) fn compile_movie(line: &ParsedLine) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &[
            "layer",
            "asset",
            "alpha",
            "loop",
            "end",
            "fence",
            "fallback",
            "interrupt",
        ],
        &["layer", "asset", "interrupt"],
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
        interrupt: parse_interrupt(line)?,
    })
}

pub(super) fn compile_audio(line: &ParsedLine, bus: VnAudioBus) -> Result<StageCommand, VnError> {
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
