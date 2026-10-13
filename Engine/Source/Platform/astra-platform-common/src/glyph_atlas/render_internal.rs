use super::*;

impl WgpuGlyphAtlasRenderer {
    pub(super) fn render_internal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        apply_mutations: bool,
        atlas_upload_query: Option<(&wgpu::QuerySet, u32, u32)>,
        timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
    ) -> Result<PreparedRender, PlatformError> {
        let profile_cpu = timestamp_query.is_some();
        let command_started = Instant::now();
        let engine_allocation_before = astra_observability::thread_allocation_snapshot();
        validate_scene_resource_payloads(&frame.commands)?;
        let resource_validation_allocation = astra_observability::thread_allocation_snapshot();
        let mut walked = walk::WalkedScene::new(self.atlas.is_none(), self.default_compositing);
        let command_setup_allocation = astra_observability::thread_allocation_snapshot();
        walked.consume(&self.resources, frame, apply_mutations)?;
        let walk::WalkedScene {
            render_mutations,
            committed_mutations,
            resources_changed,
            quad_runs,
            draw_runs,
            run_ids,
            drawn_resources,
            upload_texture_count,
            texture_region_uploads,
            upload_glyph_count,
            release_resource_count,
            transient_texture_count,
            transient_glyph_count,
            mesh_command_allocation_bytes,
            mesh_command_allocation_count,
            ..
        } = walked;
        let command_walk_allocation = astra_observability::thread_allocation_snapshot();
        let resource_view = AtlasResourceView::new(&self.resources, &render_mutations);
        validate_resource_budget(&resource_view)?;
        let command_ns = profiled_elapsed_ns(profile_cpu, command_started)?;
        let command_allocation_after = astra_observability::thread_allocation_snapshot();
        let command_allocation_bytes = command_allocation_after
            .allocated_bytes
            .saturating_sub(engine_allocation_before.allocated_bytes);
        let command_allocation_breakdown = [
            resource_validation_allocation
                .allocated_bytes
                .saturating_sub(engine_allocation_before.allocated_bytes),
            command_setup_allocation
                .allocated_bytes
                .saturating_sub(resource_validation_allocation.allocated_bytes),
            command_walk_allocation
                .allocated_bytes
                .saturating_sub(command_setup_allocation.allocated_bytes),
            command_allocation_after
                .allocated_bytes
                .saturating_sub(command_walk_allocation.allocated_bytes),
        ];
        if command_allocation_bytes > 0
            && self.last_command_allocation_breakdown != Some(command_allocation_breakdown)
            && self.command_allocation_diagnostics_emitted < 32
        {
            self.command_allocation_diagnostics_emitted += 1;
            tracing::debug!(
                event = "platform.wgpu.scene.command_allocation",
                total_bytes = command_allocation_bytes,
                resource_validation_bytes = command_allocation_breakdown[0],
                command_setup_bytes = command_allocation_breakdown[1],
                command_walk_bytes = command_allocation_breakdown[2],
                resource_budget_bytes = command_allocation_breakdown[3],
                quad_runs = quad_runs.len(),
                quad_runs_spilled = quad_runs.spilled(),
                draw_primitives = draw_runs.len(),
                draw_primitives_spilled = draw_runs.spilled(),
                run_ids = run_ids.len(),
                run_ids_spilled = run_ids.spilled(),
                drawn_resources = drawn_resources.len(),
                drawn_resources_spilled = drawn_resources.spilled(),
                render_mutations = render_mutations.len(),
                committed_mutations = committed_mutations.len(),
                upload_texture_count,
                upload_glyph_count,
                release_resource_count,
                transient_texture_count,
                transient_glyph_count,
                mesh_command_allocation_bytes,
                mesh_command_allocation_count,
                "WGPU scene command allocation profile changed"
            );
        }
        self.last_command_allocation_bytes = command_allocation_bytes;
        self.last_command_allocation_breakdown = Some(command_allocation_breakdown);

        let atlas_started = Instant::now();
        let (atlas_update, upload_bytes) = if resources_changed {
            if atlas_upload_query.is_some() {
                device
                    .poll(wgpu::PollType::Poll)
                    .map_err(|_| invalid("glyph atlas staging ring polling failed"))?;
            }
            let staging_index = self.atlas_staging_index;
            let result = update_gpu_atlas(
                AtlasUpdateContext {
                    device,
                    queue,
                    layout: &self.layout,
                    linear_sampler: &self.linear_sampler,
                    nearest_sampler: &self.nearest_sampler,
                    staging_belt: &mut self.atlas_staging_belts[staging_index],
                    timestamp_query: atlas_upload_query,
                },
                self.atlas.as_ref(),
                &self.resources,
                &resource_view,
                &texture_region_uploads,
                &mut self.atlas_upload_pixels,
            );
            if atlas_upload_query.is_some() {
                self.atlas_staging_index = 0;
            }
            result?
        } else {
            (PreparedAtlasUpdate::None, 0)
        };
        let atlas_ns = profiled_elapsed_ns(profile_cpu, atlas_started)?;
        let atlas_allocation_after = astra_observability::thread_allocation_snapshot();
        self.last_atlas_allocation_bytes = atlas_allocation_after
            .allocated_bytes
            .saturating_sub(command_allocation_after.allocated_bytes);
        self.last_upload_bytes = upload_bytes;
        let (
            active_linear_bind_group,
            active_nearest_bind_group,
            active_encoded_linear_bind_group,
            active_encoded_nearest_bind_group,
            placement_view,
        ) = match &atlas_update {
            PreparedAtlasUpdate::None => {
                let atlas = self
                    .atlas
                    .as_ref()
                    .ok_or_else(|| invalid("glyph atlas is unavailable"))?;
                (
                    &atlas.linear_bind_group,
                    &atlas.nearest_bind_group,
                    &atlas.encoded_linear_bind_group,
                    &atlas.encoded_nearest_bind_group,
                    AtlasPlacementView::committed(&atlas.packed),
                )
            }
            PreparedAtlasUpdate::Replace(atlas) => (
                &atlas.linear_bind_group,
                &atlas.nearest_bind_group,
                &atlas.encoded_linear_bind_group,
                &atlas.encoded_nearest_bind_group,
                AtlasPlacementView::committed(&atlas.packed),
            ),
            PreparedAtlasUpdate::Mutate(mutation) => {
                let atlas = self
                    .atlas
                    .as_ref()
                    .ok_or_else(|| invalid("incremental atlas update has no base atlas"))?;
                (
                    &atlas.linear_bind_group,
                    &atlas.nearest_bind_group,
                    &atlas.encoded_linear_bind_group,
                    &atlas.encoded_nearest_bind_group,
                    AtlasPlacementView::pending(&atlas.packed, &mutation.placements),
                )
            }
        };
        let geometry_started = Instant::now();
        self.vertex_bytes.clear();
        self.draw_batches.clear();
        build_vertices(
            frame,
            &quad_runs,
            &draw_runs,
            &placement_view,
            &resource_view,
            &mut self.vertex_bytes,
            &mut self.draw_batches,
        )?;
        let compositing = self
            .draw_batches
            .first()
            .map(|batch| batch.compositing)
            .unwrap_or_default();
        if self
            .draw_batches
            .iter()
            .any(|batch| batch.compositing != compositing)
        {
            return Err(invalid(
                "a scene frame cannot mix linear and encoded compositing batches",
            ));
        }
        let geometry_ns = profiled_elapsed_ns(profile_cpu, geometry_started)?;
        let engine_allocation_after = astra_observability::thread_allocation_snapshot();
        self.last_geometry_allocation_bytes = engine_allocation_after
            .allocated_bytes
            .saturating_sub(atlas_allocation_after.allocated_bytes);
        self.last_engine_allocation_bytes = engine_allocation_after
            .allocated_bytes
            .saturating_sub(engine_allocation_before.allocated_bytes);
        self.last_engine_allocation_count = engine_allocation_after
            .allocation_count
            .saturating_sub(engine_allocation_before.allocation_count);
        let vertex_upload_started = Instant::now();
        if !self.vertex_bytes.is_empty() {
            let required = self.vertex_bytes.len() as u64;
            let mut vertex_buffer_recreated = false;
            if self.vertex_capacity < required {
                let capacity = required
                    .max(4096)
                    .checked_next_power_of_two()
                    .ok_or_else(|| invalid("glyph vertex buffer capacity overflowed"))?;
                self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("astra-glyph-vertex-buffer"),
                    size: capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.vertex_capacity = capacity;
                vertex_buffer_recreated = true;
            }
            if vertex_upload_required(
                &self.vertex_bytes,
                &self.uploaded_vertex_bytes,
                vertex_buffer_recreated,
            ) {
                queue.write_buffer(
                    self.vertex_buffer
                        .as_ref()
                        .ok_or_else(|| invalid("glyph vertex buffer is unavailable"))?,
                    0,
                    &self.vertex_bytes,
                );
                self.last_upload_bytes = self
                    .last_upload_bytes
                    .checked_add(required)
                    .ok_or_else(|| invalid("GPU upload byte counter overflowed"))?;
                self.uploaded_vertex_bytes.clear();
                self.uploaded_vertex_bytes
                    .extend_from_slice(&self.vertex_bytes);
            }
        }
        let vertex_upload_ns = profiled_elapsed_ns(profile_cpu, vertex_upload_started)?;
        let render_submit_started = Instant::now();
        if self.output.as_ref().is_none_or(|output| {
            output.width != frame.width
                || output.height != frame.height
                || output.compositing != compositing
        }) {
            self.output = Some(CachedOutput {
                width: frame.width,
                height: frame.height,
                compositing,
                texture: device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("astra-glyph-output"),
                    size: wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: scene_target_format(compositing),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                }),
            });
        }
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| invalid("glyph output texture is unavailable"))?
            .texture
            .clone();
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra-glyph-render-encoder"),
        });
        {
            let clear = frame.clear_rgba;
            let clear_channel = |channel: u8| match compositing {
                SceneCompositing2D::LinearSrgb => f64::from(srgb_byte_to_linear(channel)),
                SceneCompositing2D::EncodedSrgb => f64::from(channel) / 255.0,
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-glyph-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_channel(clear[0]),
                            g: clear_channel(clear[1]),
                            b: clear_channel(clear[2]),
                            a: f64::from(clear[3]) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamp_query.map(|(query_set, beginning, end)| {
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(beginning),
                        end_of_pass_write_index: Some(end),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertex_buffer) = &self.vertex_buffer {
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for batch in &self.draw_batches {
                    pass.set_bind_group(
                        0,
                        match (batch.compositing, batch.texture_filter) {
                            (SceneCompositing2D::LinearSrgb, TextureFilter2D::Nearest) => {
                                active_nearest_bind_group
                            }
                            (SceneCompositing2D::LinearSrgb, TextureFilter2D::Linear) => {
                                active_linear_bind_group
                            }
                            (SceneCompositing2D::EncodedSrgb, TextureFilter2D::Nearest) => {
                                active_encoded_nearest_bind_group
                            }
                            (SceneCompositing2D::EncodedSrgb, TextureFilter2D::Linear) => {
                                active_encoded_linear_bind_group
                            }
                        },
                        &[],
                    );
                    pass.set_pipeline(self.pipelines.select(batch.compositing, batch.blend));
                    pass.set_scissor_rect(
                        batch.clip.x as u32,
                        batch.clip.y as u32,
                        batch.clip.width,
                        batch.clip.height,
                    );
                    pass.draw(
                        batch.first_vertex..batch.first_vertex + batch.vertex_count,
                        0..1,
                    );
                }
            }
        }
        let command_buffer = encoder.finish();
        let render_encode_ns = profiled_elapsed_ns(profile_cpu, render_submit_started)?;
        let queue_submit_started = Instant::now();
        queue.submit([command_buffer]);
        let queue_submit_ns = profiled_elapsed_ns(profile_cpu, queue_submit_started)?;
        self.last_draw_calls = self.draw_batches.len() as u64;
        let render_submit_ns = profiled_elapsed_ns(profile_cpu, render_submit_started)?;
        Ok((
            output,
            committed_mutations,
            atlas_update,
            GlyphCpuProfile {
                command_ns,
                atlas_ns,
                geometry_ns,
                vertex_upload_ns,
                render_encode_ns,
                queue_submit_ns,
                render_submit_ns,
            },
        ))
    }
}
