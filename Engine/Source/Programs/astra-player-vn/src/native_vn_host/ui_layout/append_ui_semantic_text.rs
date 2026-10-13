use super::*;

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn append_ui_semantic_text(
    localization: &VnLocalizationTable,
    measured_layouts: &BTreeMap<String, MeasuredUiTextLayout>,
    layout_ids: &mut BTreeSet<String>,
    pending: &mut Vec<PendingUiTextLayout>,
    semantics: &UiSemanticSnapshot,
    body_font_size: f32,
) -> Result<(), NativeVnHostError> {
    for node in &semantics.nodes {
        if !matches!(
            node.role,
            UiSemanticRole::Text
                | UiSemanticRole::Button
                | UiSemanticRole::Toggle
                | UiSemanticRole::Slider
                | UiSemanticRole::Select
                | UiSemanticRole::TextInput
        ) {
            continue;
        }
        let clip = ui_clip::semantic_text_clip(node)?;
        if clip.is_some_and(|clip| clip.width == 0 || clip.height == 0) {
            continue;
        }
        let Some(name) = node.name.as_deref() else {
            continue;
        };
        let text = localization
            .strings
            .get(name)
            .map(String::as_str)
            .unwrap_or(name);
        let text = match node.properties.get("text.visible_graphemes") {
            Some(value) => {
                let visible = value.parse::<usize>().map_err(|_| {
                    NativeVnHostError::Asset(
                        "ASTRA_PLAYER_UI_TEXT_REVEAL_GRAPHEME_COUNT: semantic grapheme count is invalid"
                            .into(),
                    )
                })?;
                text.graphemes(true).take(visible).collect::<String>()
            }
            None => text.to_string(),
        };
        let x = non_negative_coord(node.bounds_points.min.x, "x")?;
        let y = non_negative_coord(node.bounds_points.min.y, "y")?;
        let width =
            non_negative_coord(node.bounds_points.max.x - node.bounds_points.min.x, "width")?
                .max(1);
        let height = non_negative_coord(
            node.bounds_points.max.y - node.bounds_points.min.y,
            "height",
        )?
        .max(1);
        let direction = parse_text_direction(node.properties.get("text.direction"))?;
        let horizontal_align = parse_ui_text_alignment(node.properties.get("text.align"))?;
        let vertical_align = parse_ui_text_alignment(node.properties.get("text.vertical_align"))?;
        let max_lines = parse_bounded_u32_property(node, "text.max_lines", 4, 1, 1_024)?;
        let font_size = parse_bounded_f32_property(
            node,
            "text.font_size",
            // Blueprint text without an authored size uses Yakui's 20 px
            // default.  The renderer must shape with that exact value or its
            // cached layout no longer matches the semantic measurement.
            body_font_size,
            6.0,
            256.0,
        )?;
        let rgba = parse_ui_text_rgba(node.properties.get("text.rgba"))?;
        let measured = measured_layouts.get(&node.id).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_MISSING: Yakui did not provide the authoritative layout for {}",
                node.id
            ))
        })?;
        append_text_value(
            measured,
            layout_ids,
            pending,
            &format!("ui.text.{}", node.id),
            &text,
            x.saturating_add(8),
            y.saturating_add(8),
            width.saturating_sub(16).max(1),
            height.saturating_sub(16).max(1),
            font_size,
            max_lines,
            direction,
            rgba,
            horizontal_align,
            vertical_align,
            clip,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum UiTextAlignment {
    Start,
    Center,
    End,
}

impl UiTextAlignment {
    pub(in super::super) fn offset(self, available: u32, content: u32) -> u32 {
        let remaining = available.saturating_sub(content);
        match self {
            Self::Start => 0,
            Self::Center => remaining / 2,
            Self::End => remaining,
        }
    }
}

pub(in super::super) fn parse_ui_text_alignment(
    value: Option<&String>,
) -> Result<UiTextAlignment, NativeVnHostError> {
    match value.map(String::as_str).unwrap_or("start") {
        "start" => Ok(UiTextAlignment::Start),
        "center" => Ok(UiTextAlignment::Center),
        "end" => Ok(UiTextAlignment::End),
        _ => Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_ALIGNMENT: alignment must be start, center or end".into(),
        )),
    }
}

pub(in super::super) fn parse_ui_text_rgba(
    value: Option<&String>,
) -> Result<[u8; 4], NativeVnHostError> {
    let Some(value) = value else {
        return Ok([255; 4]);
    };
    let channels = value
        .split(',')
        .map(|channel| {
            channel.parse::<u8>().map_err(|_| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_UI_TEXT_COLOR: text.rgba must contain four u8 channels".into(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    channels.try_into().map_err(|_| {
        NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_COLOR: text.rgba must contain exactly four channels".into(),
        )
    })
}

pub(in super::super) fn ordered_ui_font_families(
    font_families: &[String],
    locale: &str,
) -> Vec<String> {
    let mut ordered = font_families.to_vec();
    ordered.sort_by_key(|family| {
        let normalized = family.to_ascii_lowercase();
        let preferred = if locale.starts_with("zh") {
            normalized.contains("sans sc")
        } else {
            normalized.contains("sans jp")
        };
        (!preferred, normalized)
    });
    ordered
}

pub(in super::super) struct NativeUiTextMeasurer {
    pub(in super::super) provider: Arc<CosmicTextLayoutProvider>,
    pub(in super::super) font_families: Arc<RwLock<Vec<String>>>,
    pub(in super::super) locale: Arc<RwLock<String>>,
    pub(in super::super) frame_layouts: Mutex<BTreeMap<String, MeasuredUiTextLayout>>,
}

#[derive(Clone)]
pub(in super::super) struct MeasuredUiTextLayout {
    pub(in super::super) text: String,
    pub(in super::super) font_size: f32,
    pub(in super::super) max_lines: u32,
    pub(in super::super) direction: TextDirection,
    pub(in super::super) layout: Arc<TextLayoutResult>,
}

impl NativeUiTextMeasurer {
    pub(in super::super) fn begin_frame(&self) -> Result<(), NativeVnHostError> {
        self.frame_layouts
            .lock()
            .map_err(|_| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_LOCK: frame layout state was poisoned".into(),
                )
            })?
            .clear();
        Ok(())
    }

    pub(in super::super) fn take_frame_layouts(
        &self,
    ) -> Result<BTreeMap<String, MeasuredUiTextLayout>, NativeVnHostError> {
        let mut layouts = self.frame_layouts.lock().map_err(|_| {
            NativeVnHostError::Asset(
                "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_LOCK: frame layout state was poisoned".into(),
            )
        })?;
        Ok(std::mem::take(&mut *layouts))
    }
}

impl AstraTextMeasurer for NativeUiTextMeasurer {
    fn measure(
        &self,
        request: &AstraTextMeasureRequest,
    ) -> Result<AstraTextMeasureResult, UiValidationError> {
        let direction = parse_text_direction(Some(&request.direction)).map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_TEXT_DIRECTION", error.to_string())
        })?;
        let locale = self.locale.read().map_err(|_| {
            UiValidationError::invalid(
                "ASTRA_UI_TEXT_LOCALE_LOCK",
                "UI text locale state was poisoned",
            )
        })?;
        let font_families = self.font_families.read().map_err(|_| {
            UiValidationError::invalid(
                "ASTRA_UI_TEXT_FONT_FALLBACK_LOCK",
                "UI font fallback state was poisoned",
            )
        })?;
        let key = format!("ui.measure.{}", request.semantic_id);
        let layout_request = text_request(TextRequestSpec {
            key: &key,
            text: &request.text,
            locale: &locale,
            font_families: &font_families,
            max_width: request.max_width.max(1.0).round() as u32,
            font_size: request.font_size,
            max_lines: request.max_lines,
            direction,
        });
        let layout = self
            .provider
            .layout_shared(&layout_request)
            .map_err(|error| {
                UiValidationError::invalid("ASTRA_UI_TEXT_MEASURE", error.to_string())
            })?;
        self.frame_layouts
            .lock()
            .map_err(|_| {
                UiValidationError::invalid(
                    "ASTRA_UI_TEXT_MEASUREMENT_LOCK",
                    "UI text frame layout state was poisoned",
                )
            })?
            .insert(
                request.semantic_id.clone(),
                MeasuredUiTextLayout {
                    text: request.text.clone(),
                    font_size: request.font_size,
                    max_lines: request.max_lines,
                    direction,
                    layout: Arc::clone(&layout),
                },
            );
        Ok(AstraTextMeasureResult {
            width: layout.width,
            height: layout.height,
        })
    }
}

pub(in super::super) fn parse_text_direction(
    value: Option<&String>,
) -> Result<TextDirection, NativeVnHostError> {
    match value.map(String::as_str).unwrap_or("auto") {
        "auto" => Ok(TextDirection::Auto),
        "left_to_right" => Ok(TextDirection::LeftToRight),
        "right_to_left" => Ok(TextDirection::RightToLeft),
        "vertical_right_to_left" => Ok(TextDirection::VerticalRightToLeft),
        "vertical_left_to_right" => Ok(TextDirection::VerticalLeftToRight),
        _ => Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_UI_TEXT_DIRECTION: semantic text direction is invalid".into(),
        )),
    }
}

pub(in super::super) fn parse_bounded_u32_property(
    node: &UiSemanticNode,
    key: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, NativeVnHostError> {
    let Some(value) = node.properties.get(key) else {
        return Ok(default);
    };
    let value = value.parse::<f32>().map_err(|_| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is not numeric"
        ))
    })?;
    if !value.is_finite() || value.fract() != 0.0 || value < min as f32 || value > max as f32 {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is outside {min}..={max}"
        )));
    }
    Ok(value as u32)
}

pub(in super::super) fn parse_bounded_f32_property(
    node: &UiSemanticNode,
    key: &str,
    default: f32,
    min: f32,
    max: f32,
) -> Result<f32, NativeVnHostError> {
    let Some(value) = node.properties.get(key) else {
        return Ok(default);
    };
    let value = value.parse::<f32>().map_err(|_| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is not numeric"
        ))
    })?;
    if !value.is_finite() || value < min || value > max {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_PROPERTY: semantic property {key} is outside {min}..={max}"
        )));
    }
    Ok(value)
}

pub(in super::super) fn non_negative_coord(
    value: f32,
    field: &str,
) -> Result<u32, NativeVnHostError> {
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f32 {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_BOUNDS: semantic {field} is outside render bounds"
        )));
    }
    Ok(value.round() as u32)
}

pub(in super::super) struct TextRequestSpec<'a> {
    pub(super) key: &'a str,
    pub(super) text: &'a str,
    pub(super) locale: &'a str,
    pub(super) font_families: &'a [String],
    pub(super) max_width: u32,
    pub(super) font_size: f32,
    pub(super) max_lines: u32,
    pub(super) direction: TextDirection,
}

pub(in super::super) fn text_request(spec: TextRequestSpec<'_>) -> TextLayoutRequest {
    let line_height = spec.font_size * 1.3;
    TextLayoutRequest {
        key: spec.key.to_string(),
        runs: vec![TextRun {
            text: spec.text.to_string(),
            language: spec.locale.to_string(),
            script: None,
            direction: spec.direction,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: spec.max_width.max(1) as f32,
            max_height: Some(line_height * spec.max_lines.max(1) as f32),
            max_lines: Some(spec.max_lines.max(1)),
            font_size: spec.font_size,
            line_height,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::EllipsisEnd,
        },
        font_families: spec.font_families.to_vec(),
        features: Vec::new(),
    }
}

pub(in super::super) fn performance_phase_started(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}

pub(in super::super) fn performance_phase_duration(
    started: Option<Instant>,
) -> Result<u64, NativeVnHostError> {
    started.map_or(Ok(0), |started| {
        u64::try_from(started.elapsed().as_nanos()).map_err(|_| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_PERFORMANCE_DURATION_OVERFLOW: UI phase exceeded u64 nanoseconds"
                    .into(),
            )
        })
    })
}

pub(in super::super) fn reusable_pointer_position(
    mut current: Option<UiPoint>,
    events: &[UiInputEvent],
    semantics: &UiSemanticSnapshot,
) -> Option<Option<UiPoint>> {
    for event in events {
        match &event.kind {
            UiInputEventKind::FixedTime { .. } => {}
            UiInputEventKind::PointerMove { position } => {
                if !same_semantic_hit_region(semantics, current, Some(*position)) {
                    return None;
                }
                current = Some(*position);
            }
            _ => return None,
        }
    }
    Some(current)
}

pub(in super::super) fn pointer_position_after_events(
    mut current: Option<UiPoint>,
    events: &[UiInputEvent],
) -> Option<UiPoint> {
    for event in events {
        match &event.kind {
            UiInputEventKind::PointerMove { position }
            | UiInputEventKind::PointerButton { position, .. } => current = Some(*position),
            UiInputEventKind::Focus { focused: false } => current = None,
            _ => {}
        }
    }
    current
}

pub(in super::super) fn same_semantic_hit_region(
    semantics: &UiSemanticSnapshot,
    before: Option<UiPoint>,
    after: Option<UiPoint>,
) -> bool {
    semantics
        .nodes
        .iter()
        .all(|node| semantic_node_contains(node, before) == semantic_node_contains(node, after))
}

pub(in super::super) fn semantic_node_contains(
    node: &UiSemanticNode,
    point: Option<UiPoint>,
) -> bool {
    point.is_some_and(|point| {
        point.x >= node.bounds_points.min.x
            && point.x < node.bounds_points.max.x
            && point.y >= node.bounds_points.min.y
            && point.y < node.bounds_points.max.y
    })
}
