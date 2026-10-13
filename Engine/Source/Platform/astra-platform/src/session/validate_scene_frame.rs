use super::*;

pub(super) fn validate_scene_frame(
    frame: &SceneFrame,
    max_bytes: usize,
) -> Result<(), PlatformError> {
    if let Some(semantics) = &frame.semantics {
        astra_ui_core::ValidateUi::validate(semantics).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "surface.present_scene",
                format!("accessibility semantic snapshot is invalid: {error}"),
            )
        })?;
    }
    let output_bytes = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if frame.sequence == 0
        || frame.width == 0
        || frame.height == 0
        || output_bytes.is_none_or(|bytes| bytes > max_bytes)
        || frame.commands.len() > max_bytes / 16
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "surface.present_scene",
            "text scene sequence, dimensions, or command count exceeds profile limits",
        ));
    }
    let mut resource_bytes = 0usize;
    let mut clip_depth = 0usize;
    let mut pixel_mask_depth = 0usize;
    let mut transform_depth = 0usize;
    let mut opacity_depth = 0usize;
    for command in &frame.commands {
        match command {
            SceneCommand::UploadTexture { frame: texture, .. } => {
                let expected = usize::try_from(texture.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(texture.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(4));
                if texture.width == 0
                    || texture.height == 0
                    || expected != Some(texture.rgba8.len())
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "texture dimensions or byte length are invalid",
                    ));
                }
                resource_bytes =
                    resource_bytes
                        .checked_add(texture.rgba8.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "scene resource byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::UpdateTextureRegion {
                width,
                height,
                rgba8,
                ..
            } => {
                let expected = usize::try_from(*width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(*height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(4));
                if *width == 0 || *height == 0 || expected != Some(rgba8.len()) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "texture region dimensions or byte length are invalid",
                    ));
                }
                resource_bytes = resource_bytes.checked_add(rgba8.len()).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "scene resource byte count overflowed",
                    )
                })?;
            }
            SceneCommand::UploadGlyph { glyph, .. } => {
                let channels = match glyph.format {
                    astra_media_core::GlyphBitmapFormat::Alpha8 => 1usize,
                    astra_media_core::GlyphBitmapFormat::Rgba8 => 4usize,
                };
                let expected = usize::try_from(glyph.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(glyph.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(channels));
                if glyph.width == 0 || glyph.height == 0 || expected != Some(glyph.pixels.len()) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "glyph bitmap dimensions or byte length are invalid",
                    ));
                }
                glyph.validate_integrity().map_err(|_| {
                    PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "glyph bitmap dimensions or byte length are invalid",
                    )
                })?;
                resource_bytes =
                    resource_bytes
                        .checked_add(glyph.pixels.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "glyph upload byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::ReleaseResource { .. } | SceneCommand::Clear { .. } => {}
            SceneCommand::GlyphRun { opacity, .. } | SceneCommand::Glyph { opacity, .. }
                if opacity.is_finite() && (0.0..=1.0).contains(opacity) => {}
            SceneCommand::Mesh2D {
                id,
                vertices,
                indices,
                material,
                texture_id,
                opacity,
                ..
            } => {
                let material_binding_valid = matches!(
                    (material, texture_id),
                    (astra_media_core::MeshMaterial2D::Solid, None)
                        | (
                            astra_media_core::MeshMaterial2D::ColorTexture
                                | astra_media_core::MeshMaterial2D::GlyphMask,
                            Some(_)
                        )
                );
                let vertices_valid = !vertices.is_empty()
                    && vertices.len() <= 250_000
                    && vertices.iter().all(|vertex| {
                        vertex.position.iter().all(|value| value.is_finite())
                            && vertex.uv.iter().all(|value| value.is_finite())
                            && vertex.premultiplied_rgba[0] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[1] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[2] <= vertex.premultiplied_rgba[3]
                    });
                let indices_valid = !indices.is_empty()
                    && indices.len() <= 750_000
                    && indices.len().is_multiple_of(3)
                    && indices
                        .iter()
                        .all(|index| (*index as usize) < vertices.len());
                if id.is_empty()
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                    || !material_binding_valid
                    || !vertices_valid
                    || !indices_valid
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "indexed mesh geometry, material, color, or opacity is invalid",
                    ));
                }
            }
            SceneCommand::MeshBatch2D {
                vertices,
                indices,
                draws,
                ..
            } => {
                let vertices_valid = !vertices.is_empty()
                    && vertices.len() <= 250_000
                    && vertices.iter().all(|vertex| {
                        vertex.position.iter().all(|value| value.is_finite())
                            && vertex.uv.iter().all(|value| value.is_finite())
                            && vertex.premultiplied_rgba[0] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[1] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[2] <= vertex.premultiplied_rgba[3]
                    });
                let draws_valid = !draws.is_empty()
                    && indices.len() <= 750_000
                    && draws.iter().all(|draw| {
                        let vertex_start = draw.vertex_start as usize;
                        let vertex_end = vertex_start.saturating_add(draw.vertex_count as usize);
                        let index_start = draw.index_start as usize;
                        let index_end = index_start.saturating_add(draw.index_count as usize);
                        let binding_valid = matches!(
                            (&draw.material, &draw.texture_id),
                            (astra_media_core::MeshMaterial2D::Solid, None)
                                | (
                                    astra_media_core::MeshMaterial2D::ColorTexture
                                        | astra_media_core::MeshMaterial2D::GlyphMask,
                                    Some(_)
                                )
                        );
                        draw.vertex_count > 0
                            && draw.index_count > 0
                            && draw.index_count.is_multiple_of(3)
                            && vertex_end <= vertices.len()
                            && index_end <= indices.len()
                            && draw.opacity.is_finite()
                            && (0.0..=1.0).contains(&draw.opacity)
                            && binding_valid
                            && indices[index_start..index_end]
                                .iter()
                                .all(|index| (*index as usize) < draw.vertex_count as usize)
                    });
                if !vertices_valid || !draws_valid {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "mesh batch geometry, material, or draw range is invalid",
                    ));
                }
            }
            SceneCommand::Rect { width, height, .. } => {
                if *width == 0 || *height == 0 {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "rectangle dimensions must be non-zero",
                    ));
                }
            }
            SceneCommand::Sprite {
                source,
                destination,
                opacity,
                ..
            } => {
                if destination.width == 0
                    || destination.height == 0
                    || source.is_some_and(|source| {
                        source.x < 0 || source.y < 0 || source.width == 0 || source.height == 0
                    })
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "sprite geometry, opacity, or blend mode is invalid",
                    ));
                }
            }
            SceneCommand::PushPixelMask { .. } => pixel_mask_depth += 1,
            SceneCommand::PopPixelMask if pixel_mask_depth > 0 => pixel_mask_depth -= 1,
            SceneCommand::PushClip { rect } => {
                if rect.width == 0 || rect.height == 0 {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "text clip dimensions must be non-zero",
                    ));
                }
                clip_depth = clip_depth.checked_add(1).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "text clip stack overflowed",
                    )
                })?;
            }
            SceneCommand::PopClip if clip_depth > 0 => clip_depth -= 1,
            SceneCommand::PopClip => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.present_scene",
                    "text clip stack underflowed",
                ));
            }
            SceneCommand::Texture {
                frame: texture,
                destination,
                opacity,
                ..
            }
            | SceneCommand::VideoFrame {
                frame: texture,
                destination,
                opacity,
                ..
            } => {
                let expected = usize::try_from(texture.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(texture.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(4));
                if texture.width == 0
                    || texture.height == 0
                    || expected != Some(texture.rgba8.len())
                    || destination.width == 0
                    || destination.height == 0
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "inline media frame geometry, opacity, or byte length is invalid",
                    ));
                }
                resource_bytes =
                    resource_bytes
                        .checked_add(texture.rgba8.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "scene resource byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::PushTransform { transform } | SceneCommand::SetCamera { transform }
                if transform.m11.is_finite()
                    && transform.m12.is_finite()
                    && transform.m21.is_finite()
                    && transform.m22.is_finite()
                    && transform.tx.is_finite()
                    && transform.ty.is_finite() =>
            {
                if matches!(command, SceneCommand::PushTransform { .. }) {
                    transform_depth = transform_depth.checked_add(1).ok_or_else(|| {
                        PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "surface.present_scene",
                            "transform stack overflowed",
                        )
                    })?;
                }
            }
            SceneCommand::PopTransform if transform_depth > 0 => transform_depth -= 1,
            SceneCommand::PushOpacity { opacity }
                if opacity.is_finite() && (0.0..=1.0).contains(opacity) =>
            {
                opacity_depth = opacity_depth.checked_add(1).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "opacity stack overflowed",
                    )
                })?;
            }
            SceneCommand::PopOpacity if opacity_depth > 0 => opacity_depth -= 1,
            SceneCommand::FilterGraph { graph }
                if graph.schema == "astra.filter_graph.v1" && !graph.nodes.is_empty() => {}
            _ => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.present_scene",
                    "scene contains an invalid renderer command or unbalanced stack operation",
                ));
            }
        }
    }
    if resource_bytes > max_bytes
        || pixel_mask_depth != 0
        || clip_depth != 0
        || transform_depth != 0
        || opacity_depth != 0
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "surface.present_scene",
            "scene resource budget or command stack is invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_package_source(
    profile: &HostLaunchProfile,
    source: &PackageSourceRequest,
) -> Result<(), PlatformError> {
    let (allowed, expected_hash) = match source {
        PackageSourceRequest::Bundled {
            relative_path,
            expected_hash,
        } => (
            profile
                .package_sources()
                .iter()
                .any(|policy| matches!(policy, PackageSourcePolicy::Bundled))
                && is_safe_relative_path(relative_path),
            expected_hash,
        ),
        PackageSourceRequest::UserAuthorized { expected_hash } => (
            profile
                .package_sources()
                .iter()
                .any(|policy| matches!(policy, PackageSourcePolicy::UserAuthorized)),
            expected_hash,
        ),
        PackageSourceRequest::HttpsRange { url, expected_hash } => {
            let origin = https_origin(url);
            let allowed = origin.as_ref().is_some_and(|origin| {
                profile.package_sources().iter().any(|policy| match policy {
                    PackageSourcePolicy::HttpsRange { allowed_origins } => {
                        allowed_origins.iter().any(|allowed| allowed == origin)
                    }
                    _ => false,
                })
            });
            (allowed, expected_hash)
        }
    };
    if !allowed {
        return Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "package.open",
            "package source is not allowed by the platform profile",
        ));
    }
    if !expected_hash.starts_with("sha256:") || expected_hash.len() <= "sha256:".len() {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.open",
            "package source requires a sha256 identity",
        ));
    }
    Ok(())
}

pub(super) fn is_safe_slot(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

pub(super) fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains('\\')
        && !value.contains("://")
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}
