use super::*;

pub(super) fn compile_timeline(line: &ParsedLine) -> Result<StageCommand, VnError> {
    validate_attrs(
        line,
        &[
            "id",
            "action",
            "reason",
            "target",
            "property",
            "keyframes",
            "easing",
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
                        easing: match line.attr("easing").unwrap_or("linear") {
                            "linear" => VnPresentationEasing::Linear,
                            "ease_in" => VnPresentationEasing::EaseIn,
                            "ease_out" => VnPresentationEasing::EaseOut,
                            "ease_in_out" => VnPresentationEasing::EaseInOut,
                            value => {
                                return Err(invalid_value(
                                    line,
                                    "easing",
                                    value,
                                    "linear,ease_in,ease_out,ease_in_out",
                                ))
                            }
                        },
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

/// Compile a Timeline authoring projection with the same validation as Cook.
pub fn compile_timeline_command(
    path: &str,
    command: &crate::AstCommand,
) -> Result<TimelineCommand, VnError> {
    let line = ParsedLine {
        source: path.into(),
        line: command.line(),
        column: command.column(),
        indent: command.indent(),
        keyword: command.keyword().into(),
        args: command
            .arguments()
            .map(|(value, _)| value.to_string())
            .collect(),
        attrs: command
            .attributes()
            .map(|a| (a.key().to_string(), a.value().to_string()))
            .collect(),
        source_id: command.source_id().map(str::to_string),
    };
    match compile_stage_command(&line)? {
        StageCommand::Timeline(timeline) => Ok(timeline),
        _ => Err(invalid_value(
            &line,
            "command",
            command.keyword(),
            "timeline",
        )),
    }
}
