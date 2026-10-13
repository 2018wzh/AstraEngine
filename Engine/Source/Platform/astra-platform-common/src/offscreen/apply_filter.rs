use super::*;

pub(super) fn apply_filter(
    context: &mut FilterPassContext<'_>,
    stage: usize,
    input: &wgpu::Texture,
    kind: &str,
    params: &BTreeMap<String, FilterParam>,
    timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
) -> Result<wgpu::Texture, PlatformError> {
    let FilterPassContext {
        device,
        queue,
        pipelines,
        outputs,
        width,
        height,
    } = context;
    let width = *width;
    let height = *height;
    let expression = match kind {
        "astra.filter.bloom" => format!(
            "vec4<f32>(min(source.rgb + vec3<f32>({}), vec3<f32>(1.0)), source.a)",
            float_param(params, "intensity")?
        ),
        "astra.filter.fade" => format!(
            "vec4<f32>(source.rgb * {}, source.a)",
            float_param(params, "amount")?
        ),
        "astra.filter.color_matrix" => format!(
            "source * vec4<f32>({}, {}, {}, {})",
            float_param(params, "r")?,
            float_param(params, "g")?,
            float_param(params, "b")?,
            float_param(params, "a")?
        ),
        _ => {
            return Err(invalid(
                "offscreen.filter",
                "GPU filter kind is unsupported",
            ))
        }
    };
    let signature = hash(format!("{kind}:{expression}").as_bytes());
    if !pipelines.contains_key(&signature) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("astra-offscreen-filter"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@group(0) @binding(0) var input_texture: texture_2d<f32>;\n\
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{\n\
                   let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));\n\
                   return vec4<f32>(p[i], 0.0, 1.0);\n\
                 }}\n\
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n\
                   let source = textureLoad(input_texture, vec2<i32>(p.xy), 0);\n\
                   return {expression};\n\
                 }}"
                )
                .into(),
            ),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra-offscreen-filter-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("astra-offscreen-filter-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("astra-offscreen-filter-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        pipelines.insert(
            signature.clone(),
            CachedFilterPipeline {
                bind_layout,
                pipeline,
            },
        );
    }
    let cached = pipelines
        .get(&signature)
        .ok_or_else(|| invalid("offscreen.filter", "filter pipeline cache is unavailable"))?;
    while outputs.len() <= stage {
        outputs.push(CachedFilterOutput {
            width: 0,
            height: 0,
            texture: create_filter_output(device, 1, 1),
            binding_signature: None,
            bind_group: None,
        });
    }
    if outputs[stage].width != width || outputs[stage].height != height {
        outputs[stage] = CachedFilterOutput {
            width,
            height,
            texture: create_filter_output(device, width, height),
            binding_signature: None,
            bind_group: None,
        };
        for downstream in &mut outputs[stage + 1..] {
            downstream.binding_signature = None;
            downstream.bind_group = None;
        }
    }
    let output = outputs[stage].texture.clone();
    if outputs[stage].binding_signature.as_deref() != Some(signature.as_str()) {
        let input_view = input.create_view(&Default::default());
        outputs[stage].bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-offscreen-filter-bind-group"),
            layout: &cached.bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_view),
            }],
        }));
        outputs[stage].binding_signature = Some(signature.clone());
    }
    let output_view = output.create_view(&Default::default());
    let bind_group = outputs[stage]
        .bind_group
        .as_ref()
        .ok_or_else(|| invalid("offscreen.filter", "filter bind group cache is unavailable"))?;
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("astra-offscreen-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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
        pass.set_pipeline(&cached.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    Ok(output)
}

pub(super) fn mark_gpu_timestamp_recorded(
    timer: &mut GpuTimer,
    timer_slot: usize,
    query_count: u32,
) -> Result<(), PlatformError> {
    let slot = timer.slots.get_mut(timer_slot).ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot is outside the bounded ring",
        )
    })?;
    if slot.pending.is_some() {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot was reused before resolution",
        ));
    }
    slot.pending = Some(PendingTimestampRead::Recorded { query_count });
    Ok(())
}

pub(super) fn submit_recorded_timestamp_reads(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
) -> Result<(), PlatformError> {
    let recorded = timer
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| match slot.pending.as_ref() {
            Some(PendingTimestampRead::Recorded { query_count }) => Some((index, *query_count)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if recorded.is_empty() {
        return Err(invalid(
            "offscreen.timestamp_query",
            "timestamp resolve batch contains no recorded slots",
        ));
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("astra-offscreen-performance-resolve-encoder"),
    });
    for (index, query_count) in &recorded {
        let slot = &timer.slots[*index];
        let byte_length = u64::from(*query_count) * 8;
        encoder.resolve_query_set(&slot.query_set, 0..*query_count, &slot.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve_buffer, 0, &slot.read_buffer, 0, byte_length);
    }
    let submission = queue.submit([encoder.finish()]);
    timer.resolve_submissions_since_frame += 1;
    for (index, query_count) in recorded {
        let slot = &mut timer.slots[index];
        let byte_length = u64::from(query_count) * 8;
        let slice = slot.read_buffer.slice(..byte_length);
        slot.map_status.store(GPU_MAP_PENDING, Ordering::Release);
        let map_status = Arc::clone(&slot.map_status);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            map_status.store(
                if result.is_ok() {
                    GPU_MAP_SUCCEEDED
                } else {
                    GPU_MAP_FAILED
                },
                Ordering::Release,
            );
        });
        slot.pending = Some(PendingTimestampRead::Submitted {
            submission: submission.clone(),
            query_count,
        });
    }
    Ok(())
}

pub(super) fn resolve_gpu_timestamps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
    timer_slot: usize,
    query_count: u32,
    profiled_atlas_upload: bool,
    wait: bool,
) -> Result<Option<(u64, u64, u64)>, PlatformError> {
    let timestamp_period_ns = timer.timestamp_period_ns;
    let recorded_query_count =
        timer
            .slots
            .get(timer_slot)
            .and_then(|slot| match slot.pending.as_ref() {
                Some(PendingTimestampRead::Recorded { query_count }) => Some(*query_count),
                _ => None,
            });
    if let Some(recorded_query_count) = recorded_query_count {
        if recorded_query_count != query_count {
            return Err(invalid(
                "offscreen.timestamp_query",
                "GPU timestamp query count changed before resolution",
            ));
        }
        submit_recorded_timestamp_reads(device, queue, timer)?;
    }
    let slot = timer.slots.get_mut(timer_slot).ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot is outside the bounded ring",
        )
    })?;
    let pending = slot.pending.as_ref().ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp submission was already resolved",
        )
    })?;
    let PendingTimestampRead::Submitted {
        submission,
        query_count: pending_query_count,
    } = pending
    else {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp resolve batch was not submitted",
        ));
    };
    if *pending_query_count != query_count {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp query count changed before resolution",
        ));
    }
    if wait {
        let callback_deadline = Instant::now() + READBACK_TIMEOUT;
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.clone()),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|_| {
                unavailable(
                    "offscreen.timestamp_query",
                    "GPU timestamp read exceeded its bounded timeout",
                )
            })?;
        loop {
            match slot.map_status.load(Ordering::Acquire) {
                GPU_MAP_SUCCEEDED => break,
                GPU_MAP_FAILED => {
                    return Err(unavailable(
                        "offscreen.timestamp_query",
                        "GPU timestamp map failed",
                    ));
                }
                GPU_MAP_PENDING if Instant::now() < callback_deadline => {
                    device.poll(wgpu::PollType::Poll).map_err(|_| {
                        unavailable(
                            "offscreen.timestamp_query",
                            "GPU timestamp callback polling failed",
                        )
                    })?;
                    thread::yield_now();
                }
                GPU_MAP_PENDING => {
                    return Err(unavailable(
                        "offscreen.timestamp_query",
                        "GPU timestamp map callback exceeded its bounded timeout",
                    ));
                }
                _ => unreachable!("GPU timestamp callback uses a closed status set"),
            }
        }
    } else {
        // `map_async` callbacks advance only while the device is polled.  The
        // performance runner deliberately uses this non-blocking path before
        // the bounded readback ring fills; without a poll, every slot stays
        // pending until the ring forces a synchronous wait, creating periodic
        // multi-frame CPU stalls on integrated DX12 adapters.
        device.poll(wgpu::PollType::Poll).map_err(|_| {
            unavailable(
                "offscreen.timestamp_query",
                "GPU timestamp callback polling failed",
            )
        })?;
        match slot.map_status.load(Ordering::Acquire) {
            GPU_MAP_SUCCEEDED => {}
            GPU_MAP_PENDING => return Ok(None),
            GPU_MAP_FAILED => {
                return Err(unavailable(
                    "offscreen.timestamp_query",
                    "GPU timestamp map failed",
                ));
            }
            _ => unreachable!("GPU timestamp callback uses a closed status set"),
        }
    }
    slot.pending.take().ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp submission was already resolved",
        )
    })?;
    let byte_length = u64::from(query_count) * 8;
    let slice = slot.read_buffer.slice(..byte_length);
    let mapped = slice.get_mapped_range();
    let base_query_count = 2 + u32::from(profiled_atlas_upload) * 2;
    if query_count < base_query_count || !(query_count - base_query_count).is_multiple_of(2) {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp query layout is invalid",
        ));
    }
    let timestamp = |index: usize| {
        let offset = index * 8;
        u64::from_le_bytes(
            mapped[offset..offset + 8]
                .try_into()
                .expect("timestamp bytes"),
        )
    };
    let duration = |begin: u64, end: u64| -> Result<u64, PlatformError> {
        let ticks = end
            .checked_sub(begin)
            .ok_or_else(|| invalid("offscreen.timestamp_query", "GPU timestamp moved backwards"))?;
        let duration_ns = ticks as f64 * f64::from(timestamp_period_ns);
        if !duration_ns.is_finite() || duration_ns < 0.0 || duration_ns > u64::MAX as f64 {
            return Err(invalid(
                "offscreen.timestamp_query",
                "GPU timestamp duration is invalid",
            ));
        }
        Ok(duration_ns.round() as u64)
    };
    let atlas_upload_gpu_ns = if profiled_atlas_upload {
        duration(timestamp(0), timestamp(1))?
    } else {
        0
    };
    let scene_begin = usize::from(profiled_atlas_upload) * 2;
    let scene_gpu_ns = duration(timestamp(scene_begin), timestamp(scene_begin + 1))?;
    let mut filter_gpu_ns = 0u64;
    let filter_base = scene_begin + 2;
    for stage in 0..(query_count as usize - filter_base) / 2 {
        let begin = timestamp(filter_base + stage * 2);
        let end = timestamp(filter_base + stage * 2 + 1);
        filter_gpu_ns = filter_gpu_ns
            .checked_add(duration(begin, end)?)
            .ok_or_else(|| invalid("offscreen.timestamp_query", "GPU duration overflowed"))?;
    }
    drop(mapped);
    slot.read_buffer.unmap();
    Ok(Some((atlas_upload_gpu_ns, scene_gpu_ns, filter_gpu_ns)))
}

pub(super) fn elapsed_ns(started: Instant, operation: &'static str) -> Result<u64, PlatformError> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid(operation, "duration overflowed"))
}

pub(super) fn create_filter_output(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-offscreen-filter-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
