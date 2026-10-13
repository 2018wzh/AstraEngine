use super::*;

pub(in super::super) fn stage_scene_commands(
    state: &ProductStageState,
    textures: &BTreeMap<String, TextureFrame>,
    texture_dimensions: &BTreeMap<String, (u32, u32)>,
    width: u32,
    height: u32,
) -> Result<Vec<SceneCommand>, NativeVnHostError> {
    let mut commands = Vec::new();
    let rotated = state.camera.rotation.millionths != 0;
    if rotated {
        let radians = (state.camera.rotation.millionths as f32 / 1_000_000.0).to_radians();
        let (sin, cos) = radians.sin_cos();
        let center_x = width as f32 / 2.0;
        let center_y = height as f32 / 2.0;
        commands.push(SceneCommand::PushTransform {
            transform: Transform2D {
                m11: cos,
                m12: sin,
                m21: -sin,
                m22: cos,
                tx: center_x - cos * center_x + sin * center_y,
                ty: center_y - sin * center_x - cos * center_y,
            },
        });
    }
    if let Some(color) = state.backdrop_color {
        commands.push(SceneCommand::rect(
            "vn.scene.backdrop",
            0,
            0,
            width,
            height,
            color,
        ));
    }
    let mut entities = state
        .entities
        .values()
        .filter(|entity| {
            entity.visible
                && state
                    .layers
                    .get(&entity.layer)
                    .is_some_and(|layer| layer.visible)
        })
        .collect::<Vec<_>>();
    entities.sort_by(|left, right| {
        let left_z = state
            .layers
            .get(&left.layer)
            .map_or(i32::MIN, |layer| layer.z);
        let right_z = state
            .layers
            .get(&right.layer)
            .map_or(i32::MIN, |layer| layer.z);
        left_z.cmp(&right_z).then(left.id.cmp(&right.id))
    });
    for entity in entities {
        let layer = state.layers.get(&entity.layer).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_STAGE_LAYER_UNKNOWN: entity {} references an undeclared layer",
                entity.id
            ))
        })?;
        let (texture_width, texture_height) = texture_dimensions
            .get(&entity.asset)
            .ok_or_else(|| missing_texture(&entity.asset))?;
        let clip = match layer.clip {
            Some(StageClipPolicy::SafeArea) => Some(safe_area_rect(
                width,
                height,
                state.safe_area.width,
                state.safe_area.height,
            )?),
            Some(StageClipPolicy::Stage) => Some(RectI::new(0, 0, width, height)),
            None => None,
        };
        if let Some(clip) = clip {
            commands.push(SceneCommand::PushClip { rect: clip });
        }
        let destination = if layer.kind == StageLayerKind::Background {
            RectI::new(0, 0, width, height)
        } else {
            entity_destination(
                state,
                *texture_width,
                *texture_height,
                entity.fit,
                entity.x.millionths,
                entity.y.millionths,
                width,
                height,
            )?
        };
        commands.push(SceneCommand::Sprite {
            id: format!("vn.scene.{}.{}", entity.layer, entity.id),
            texture_id: entity.asset.clone(),
            source: None,
            destination,
            opacity: fixed_opacity(entity.opacity.millionths)?,
            blend: stage_blend(layer.blend)?,
        });
        if clip.is_some() {
            commands.push(SceneCommand::PopClip);
        }
    }
    if state.shade_opacity.millionths > 0 {
        let shade_alpha = u8::try_from(
            state
                .shade_opacity
                .millionths
                .checked_mul(255)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_SHADE_ALPHA_OVERFLOW".into())
                })?
                / 1_000_000,
        )
        .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_SHADE_ALPHA_RANGE".into()))?;
        commands.push(SceneCommand::rect(
            "vn.scene.shade",
            0,
            0,
            width,
            height,
            [
                state.shade_color[0],
                state.shade_color[1],
                state.shade_color[2],
                shade_alpha,
            ],
        ));
    }
    let mut movies = state.movies.values().collect::<Vec<_>>();
    movies.sort_by(|left, right| {
        let left_z = state
            .layers
            .get(&left.layer)
            .map_or(i32::MIN, |layer| layer.z);
        let right_z = state
            .layers
            .get(&right.layer)
            .map_or(i32::MIN, |layer| layer.z);
        left_z.cmp(&right_z).then(left.layer.cmp(&right.layer))
    });
    for movie in movies {
        let layer = state.layers.get(&movie.layer).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_STAGE_LAYER_UNKNOWN: movie {} references an undeclared layer",
                movie.asset
            ))
        })?;
        let frame = if let Some(frame) = textures.get(&movie.asset) {
            frame
        } else {
            let fallback = movie
                .fallback
                .as_deref()
                .ok_or_else(|| missing_texture(&movie.asset))?;
            textures
                .get(fallback)
                .ok_or_else(|| missing_texture(fallback))?
        };
        commands.push(SceneCommand::VideoFrame {
            id: format!("vn.movie.{}", movie.layer),
            frame: frame.clone(),
            destination: RectI::new(0, 0, width, height),
            opacity: fixed_opacity(movie.alpha.millionths)?,
            blend: stage_blend(layer.blend)?,
            presentation_time_ns: state.elapsed_ns,
        });
    }
    let mut effects = state.effects.values().collect::<Vec<_>>();
    effects.sort_by(|left, right| left.target.cmp(&right.target));
    for effect in effects {
        commands.push(SceneCommand::FilterGraph {
            graph: effect_filter_graph(effect)?,
        });
    }
    if rotated {
        commands.push(SceneCommand::PopTransform);
    }
    Ok(commands)
}

pub(in super::super) fn compose_director_transition_scene(
    snapshot: &DirectorTransitionSnapshot,
    next: &[SceneCommand],
    progress_millionths: i64,
    width: u32,
    height: u32,
) -> Result<Vec<SceneCommand>, NativeVnHostError> {
    let progress = progress_millionths.clamp(0, 1_000_000) as u64;
    // Scene command identities are frame-local.  A Director transition draws
    // the outgoing scene and one or more clipped instances of the incoming
    // scene in the same frame, so their otherwise stable stage ids must be
    // namespaced by their transition role.  Resource ids deliberately remain
    // unchanged: they name retained GPU resources rather than draw instances.
    let mut commands = namespace_transition_scene_draw(&snapshot.source_draw, "source")?;
    if snapshot.descriptor_id == "director.puppet.26" {
        commands.push(SceneCommand::PushPixelMask {
            bits: dissolve_pattern(progress)?,
        });
        commands.extend(namespace_transition_scene_draw(next, "incoming")?);
        commands.push(SceneCommand::PopPixelMask);
        return Ok(commands);
    }
    let rectangles = match snapshot.descriptor_id.as_str() {
        "director.puppet.1" => vec![RectI::new(
            0,
            0,
            u32::try_from(u64::from(width) * progress / 1_000_000).map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
            })?,
            height,
        )],
        "director.puppet.9" => {
            let reveal_width =
                u32::try_from(u64::from(width) * progress / 1_000_000).map_err(|_| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
                })?;
            let reveal_height =
                u32::try_from(u64::from(height) * progress / 1_000_000).map_err(|_| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
                })?;
            vec![RectI::new(
                i32::try_from((width - reveal_width) / 2).map_err(|_| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
                })?,
                i32::try_from((height - reveal_height) / 2).map_err(|_| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
                })?,
                reveal_width,
                reveal_height,
            )]
        }
        "director.puppet.10" => edge_in_transition_rectangles(progress, width, height)?,
        _ => {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_DIRECTOR_TRANSITION_DESCRIPTOR_UNSUPPORTED".into(),
            ));
        }
    };
    for (clip_index, rectangle) in rectangles.into_iter().enumerate() {
        if rectangle.width == 0 || rectangle.height == 0 {
            continue;
        }
        commands.push(SceneCommand::PushClip { rect: rectangle });
        commands.extend(namespace_transition_scene_draw(
            next,
            &format!("incoming.{clip_index}"),
        )?);
        commands.push(SceneCommand::PopClip);
    }
    Ok(commands)
}

pub(in super::super) fn namespace_transition_scene_draw(
    draw: &[SceneCommand],
    namespace: &str,
) -> Result<Vec<SceneCommand>, NativeVnHostError> {
    if namespace.is_empty() {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_DIRECTOR_TRANSITION_NAMESPACE_EMPTY".into(),
        ));
    }
    let mut namespaced = draw.to_vec();
    for command in &mut namespaced {
        let id = match command {
            SceneCommand::Sprite { id, .. }
            | SceneCommand::GlyphRun { id, .. }
            | SceneCommand::Mesh2D { id, .. }
            | SceneCommand::Rect { id, .. }
            | SceneCommand::Texture { id, .. }
            | SceneCommand::VideoFrame { id, .. }
            | SceneCommand::Glyph { id, .. } => Some(id),
            _ => None,
        };
        if let Some(id) = id {
            let namespaced_id = format!("vn.transition.{namespace}.{id}");
            if namespaced_id.len() > 256 {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_DIRECTOR_TRANSITION_COMMAND_ID_TOO_LONG".into(),
                ));
            }
            *id = namespaced_id;
        }
    }
    Ok(namespaced)
}

pub(in super::super) fn save_director_transition_snapshot(
    snapshot: Option<&DirectorTransitionSnapshot>,
) -> Result<Option<SavedDirectorTransitionSnapshot>, NativeVnHostError> {
    Ok(snapshot.map(|snapshot| SavedDirectorTransitionSnapshot {
        descriptor_id: snapshot.descriptor_id.clone(),
        source_state: snapshot.source_state.clone(),
    }))
}

pub(in super::super) fn restore_director_transition_snapshot(
    snapshot: Option<&SavedDirectorTransitionSnapshot>,
    textures: &BTreeMap<String, TextureFrame>,
    texture_dimensions: &BTreeMap<String, (u32, u32)>,
    width: u32,
    height: u32,
) -> Result<Option<DirectorTransitionSnapshot>, NativeVnHostError> {
    snapshot
        .map(|snapshot| {
            let source_draw = stage_scene_commands(
                &snapshot.source_state,
                textures,
                texture_dimensions,
                width,
                height,
            )?;
            Ok(DirectorTransitionSnapshot {
                descriptor_id: snapshot.descriptor_id.clone(),
                source_state: snapshot.source_state.clone(),
                source_draw,
            })
        })
        .transpose()
}

pub(in super::super) fn edge_in_transition_rectangles(
    progress: u64,
    width: u32,
    height: u32,
) -> Result<Vec<RectI>, NativeVnHostError> {
    let x_band = u32::try_from(u64::from(width) * progress / 2_000_000)
        .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into()))?;
    let y_band = u32::try_from(u64::from(height) * progress / 2_000_000)
        .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into()))?;
    let middle_height = height.saturating_sub(y_band.saturating_mul(2));
    let right_x = width.saturating_sub(x_band);
    Ok(vec![
        RectI::new(0, 0, width, y_band),
        RectI::new(
            0,
            i32::try_from(height.saturating_sub(y_band)).map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
            })?,
            width,
            y_band,
        ),
        RectI::new(
            0,
            i32::try_from(y_band).map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
            })?,
            x_band,
            middle_height,
        ),
        RectI::new(
            i32::try_from(right_x).map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
            })?,
            i32::try_from(y_band).map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into())
            })?,
            x_band,
            middle_height,
        ),
    ])
}

pub(in super::super) fn dissolve_pattern(progress: u64) -> Result<u64, NativeVnHostError> {
    // Director's type 26 is not a random dissolve.  It reveals the next frame
    // through one of these ordered 8x8 bit patterns, tiled at native pixel
    // resolution.  Keeping the pattern table in the executor (rather than
    // treating it as a generic fade) makes the descriptor auditable and avoids
    // a silently approximate transition implementation.
    const PATTERNS: [u64; 64] = [
        0x8000000000000000,
        0x8000000008000000,
        0x8800000008000000,
        0x8800000088000000,
        0x8800200088000000,
        0x8800200088000200,
        0x8800220088000200,
        0x8800220088002200,
        0xa800220088002200,
        0xa80022008a002200,
        0xaa0022008a002200,
        0xaa002200aa002200,
        0xaa00a200aa002200,
        0xaa00a200aa002a00,
        0xaa00aa00aa002a00,
        0xaa00aa00aa00aa00,
        0xaa40aa00aa00aa00,
        0xaa40aa00aa04aa00,
        0xaa44aa00aa04aa00,
        0xaa44aa00aa44aa00,
        0xaa44aa10aa44aa00,
        0xaa44aa10aa44aa01,
        0xaa44aa11aa44aa01,
        0xaa44aa11aa44aa11,
        0xaa54aa11aa44aa11,
        0xaa54aa11aa45aa11,
        0xaa55aa11aa45aa11,
        0xaa55aa11aa55aa11,
        0xaa55aa51aa55aa11,
        0xaa55aa51aa55aa15,
        0xaa55aa55aa55aa15,
        0xaa55aa55aa55aa55,
        0xea55aa55aa55aa55,
        0xea55aa55ae55aa55,
        0xee55aa55ae55aa55,
        0xee55aa55ee55aa55,
        0xee55ba55ee55aa55,
        0xee55ba55ee55ab55,
        0xee55bb55ee55ab55,
        0xee55bb55ee55bb55,
        0xfe55bb55ee55bb55,
        0xfe55bb55ef55bb55,
        0xff55bb55ef55bb55,
        0xff55bb55ff55bb55,
        0xff55fb55ff55bb55,
        0xff55fb55ff55bf55,
        0xff55ff55ff55bf55,
        0xff55ff55ff55ff55,
        0xffd5ff55ff55ff55,
        0xffd5ff55ff5dff55,
        0xffddff55ff5dff55,
        0xffddff55ffddff55,
        0xffddff75ffddff55,
        0xffddff75ffddff57,
        0xffddff77ffddff57,
        0xffddff77ffddff77,
        0xfffdff77ffddff77,
        0xfffdff77ffdfff77,
        0xffffff77ffdfff77,
        0xffffff77ffffff77,
        0xfffffff7ffffff77,
        0xfffffff7ffffff7f,
        0xffffffffffffff7f,
        0xffffffffffffffff,
    ];
    let pattern_index = usize::try_from(progress.saturating_mul(63) / 1_000_000)
        .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_DIRECTOR_TRANSITION_RANGE".into()))?;
    Ok(PATTERNS[pattern_index])
}
