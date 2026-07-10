use astra_platform::{HostStartFuture, PlatformHostFactory, PlatformHostProfile};

#[cfg(not(target_arch = "wasm32"))]
use astra_platform::{PlatformError, PlatformErrorCode};

#[derive(Debug, Clone, Copy, Default)]
pub struct WebPlatformFactory;

pub fn factory() -> WebPlatformFactory {
    WebPlatformFactory
}

impl PlatformHostFactory for WebPlatformFactory {
    fn start(&self, profile: PlatformHostProfile) -> HostStartFuture {
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(browser::start(profile))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(async move {
                let _ = profile;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Web host requires a wasm32 browser environment",
                ))
            })
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::collections::BTreeMap;

    use astra_platform::{
        host_channel, CapturedFrame, HostCommand, PlatformBackendChannels, PlatformError,
        PlatformErrorCode, PlatformEventKind, PlatformHostProfile, PlatformHostSession, RgbaFrame,
        SurfaceHandle, WindowHandle,
    };
    use astra_platform_general::ResourceTable;
    use wasm_bindgen::{closure::Closure, JsCast};
    use wasm_bindgen_futures::spawn_local;
    use web_sys::{Event, HtmlCanvasElement, KeyboardEvent, PointerEvent, WheelEvent};

    pub async fn start(profile: PlatformHostProfile) -> Result<PlatformHostSession, PlatformError> {
        if profile.platform != astra_platform::PlatformId::Web {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "host.start",
                "Web factory requires a Web profile",
            ));
        }
        let (client, backend, events) = host_channel(
            profile.clone(),
            profile.limits.command_queue_capacity,
            profile.limits.event_queue_capacity,
        )?;
        backend.event_emitter().emit(PlatformEventKind::Resumed)?;
        spawn_local(run_backend(backend));
        Ok(PlatformHostSession {
            client,
            events,
            profile,
        })
    }

    async fn run_backend(mut backend: PlatformBackendChannels) {
        let emitter = backend.event_emitter();
        let mut windows = ResourceTable::<CanvasResource, WindowHandle>::new("window");
        let mut surfaces = ResourceTable::<SurfaceResource, SurfaceHandle>::new("surface");
        while let Some(command) = backend.next_command().await {
            match command {
                HostCommand::CreateWindow { request, reply } => {
                    let result = CanvasResource::new(
                        request.title,
                        request.width,
                        request.height,
                        request.visible,
                    )
                    .and_then(|resource| windows.insert(resource))
                    .and_then(|handle| {
                        windows
                            .get_mut(handle)?
                            .bind_events(handle, emitter.clone())?;
                        Ok(handle)
                    });
                    let _ = reply.send(result);
                }
                HostCommand::CreateSurface { request, reply } => {
                    let result = match windows.get(request.window) {
                        Ok(window) => SurfaceResource::new(
                            window.canvas.clone(),
                            request.width,
                            request.height,
                        )
                        .await
                        .and_then(|surface| surfaces.insert(surface)),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::PresentRgba {
                    surface,
                    frame,
                    reply,
                } => {
                    let result = match surfaces.get_mut(surface) {
                        Ok(surface) => surface.present(frame),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::CaptureSurface { surface, reply } => {
                    let result = match surfaces.get_mut(surface) {
                        Ok(surface) => surface.capture().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::DestroySurface { surface, reply } => {
                    let _ = reply.send(surfaces.remove(surface).map(|_| ()));
                }
                HostCommand::DestroyWindow { window, reply } => {
                    let _ = reply.send(windows.remove(window).map(|_| ()));
                }
                HostCommand::Shutdown { reply } => {
                    let result = surfaces.ensure_empty().and_then(|_| windows.ensure_empty());
                    let exit = result.is_ok();
                    let _ = reply.send(result);
                    if exit {
                        break;
                    }
                }
                other => {
                    let operation = other.operation();
                    let _ = other.reply_unit(Err(PlatformError::new(
                        PlatformErrorCode::ProviderUnavailable,
                        operation,
                        "Web host service is not initialized",
                    )));
                }
            }
        }
    }

    struct CanvasResource {
        canvas: HtmlCanvasElement,
        listeners: BTreeMap<&'static str, Closure<dyn FnMut(Event)>>,
    }

    impl CanvasResource {
        fn new(
            title: String,
            width: u32,
            height: u32,
            visible: bool,
        ) -> Result<Self, PlatformError> {
            let window = web_sys::window().ok_or_else(|| web_error("window.create"))?;
            let document = window
                .document()
                .ok_or_else(|| web_error("window.create"))?;
            let canvas = document
                .create_element("canvas")
                .map_err(|_| web_error("window.create"))?
                .dyn_into::<HtmlCanvasElement>()
                .map_err(|_| web_error("window.create"))?;
            canvas.set_width(width);
            canvas.set_height(height);
            canvas.set_tab_index(0);
            canvas.set_id("astra-player-canvas");
            canvas
                .set_attribute("aria-label", &title)
                .map_err(|_| web_error("window.create"))?;
            if !visible {
                canvas
                    .style()
                    .set_property("display", "none")
                    .map_err(|_| web_error("window.create"))?;
            }
            document
                .body()
                .ok_or_else(|| web_error("window.create"))?
                .append_child(&canvas)
                .map_err(|_| web_error("window.create"))?;
            Ok(Self {
                canvas,
                listeners: BTreeMap::new(),
            })
        }

        fn bind_events(
            &mut self,
            window: WindowHandle,
            emitter: astra_platform::PlatformEventEmitter,
        ) -> Result<(), PlatformError> {
            self.add_listener("keydown", {
                let emitter = emitter.clone();
                move |event| {
                    if let Ok(event) = event.dyn_into::<KeyboardEvent>() {
                        let _ = emitter.emit(PlatformEventKind::Keyboard {
                            window,
                            physical_key: event.code(),
                            logical_key: Some(event.key()),
                            state: astra_platform::InputState::Pressed,
                            repeat: event.repeat(),
                        });
                    }
                }
            })?;
            self.add_listener("keyup", {
                let emitter = emitter.clone();
                move |event| {
                    if let Ok(event) = event.dyn_into::<KeyboardEvent>() {
                        let _ = emitter.emit(PlatformEventKind::Keyboard {
                            window,
                            physical_key: event.code(),
                            logical_key: Some(event.key()),
                            state: astra_platform::InputState::Released,
                            repeat: event.repeat(),
                        });
                    }
                }
            })?;
            self.add_listener("pointermove", {
                let emitter = emitter.clone();
                move |event| {
                    if let Ok(event) = event.dyn_into::<PointerEvent>() {
                        let _ = emitter.emit(PlatformEventKind::PointerMoved {
                            window,
                            x: event.offset_x() as f64,
                            y: event.offset_y() as f64,
                        });
                    }
                }
            })?;
            self.add_listener("wheel", move |event| {
                if let Ok(event) = event.dyn_into::<WheelEvent>() {
                    let _ = emitter.emit(PlatformEventKind::MouseWheel {
                        window,
                        delta_x: event.delta_x() as f32,
                        delta_y: event.delta_y() as f32,
                    });
                }
            })?;
            Ok(())
        }

        fn add_listener(
            &mut self,
            name: &'static str,
            callback: impl FnMut(Event) + 'static,
        ) -> Result<(), PlatformError> {
            let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
            self.canvas
                .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
                .map_err(|_| web_error("input.bind"))?;
            self.listeners.insert(name, callback);
            Ok(())
        }
    }

    impl Drop for CanvasResource {
        fn drop(&mut self) {
            for (name, callback) in &self.listeners {
                let _ = self
                    .canvas
                    .remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
            }
            self.canvas.remove();
        }
    }

    struct SurfaceResource {
        _instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        bind_group_layout: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
        pipeline: wgpu::RenderPipeline,
        last_upload: Option<UploadFrame>,
    }

    impl SurfaceResource {
        async fn new(
            canvas: HtmlCanvasElement,
            width: u32,
            height: u32,
        ) -> Result<Self, PlatformError> {
            let instance = wgpu::Instance::default();
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|_| web_error("surface.create"))?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                })
                .await
                .map_err(|_| web_error("surface.create"))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .map_err(|_| web_error("surface.create"))?;
            let mut config = surface
                .get_default_config(&adapter, width, height)
                .ok_or_else(|| web_error("surface.create"))?;
            config.present_mode = wgpu::PresentMode::Fifo;
            surface.configure(&device, &config);
            let (bind_group_layout, sampler, pipeline) = create_pipeline(&device, config.format);
            Ok(Self {
                _instance: instance,
                surface,
                _adapter: adapter,
                device,
                queue,
                config,
                bind_group_layout,
                sampler,
                pipeline,
                last_upload: None,
            })
        }

        fn present(&mut self, frame: RgbaFrame) -> Result<(), PlatformError> {
            if frame.width != self.config.width || frame.height != self.config.height {
                self.config.width = frame.width;
                self.config.height = frame.height;
                self.surface.configure(&self.device, &self.config);
            }
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra-web-frame-upload"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba8,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra-web-frame-bind-group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let output = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(output)
                | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                    return Err(PlatformError::new(
                        PlatformErrorCode::ContextLost,
                        "surface.present_rgba",
                        "WebGPU canvas surface was lost",
                    ));
                }
                _ => return Err(web_error("surface.present_rgba")),
            };
            let output_view = output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-web-frame-encoder"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("astra-web-frame-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &output_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            self.queue.submit([encoder.finish()]);
            self.queue.present(output);
            self.last_upload = Some(UploadFrame {
                texture,
                width: frame.width,
                height: frame.height,
            });
            Ok(())
        }

        async fn capture(&mut self) -> Result<CapturedFrame, PlatformError> {
            let upload = self
                .last_upload
                .as_ref()
                .ok_or_else(|| web_error("surface.capture"))?;
            let row = upload.width * 4;
            let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("astra-web-frame-readback"),
                size: u64::from(padded) * u64::from(upload.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-web-frame-readback-encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &upload.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(upload.height),
                    },
                },
                wgpu::Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);
            let (mapped_tx, mapped_rx) = tokio::sync::oneshot::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = mapped_tx.send(result);
                });
            mapped_rx
                .await
                .map_err(|_| web_error("surface.capture"))?
                .map_err(|_| web_error("surface.capture"))?;
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|_| web_error("surface.capture"))?;
            let mut rgba8 = Vec::with_capacity((row * upload.height) as usize);
            for bytes in mapped
                .chunks_exact(padded as usize)
                .take(upload.height as usize)
            {
                rgba8.extend_from_slice(&bytes[..row as usize]);
            }
            drop(mapped);
            buffer.unmap();
            Ok(CapturedFrame {
                width: upload.width,
                height: upload.height,
                rgba8,
            })
        }
    }

    struct UploadFrame {
        texture: wgpu::Texture,
        width: u32,
        height: u32,
    }

    fn create_pipeline(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> (wgpu::BindGroupLayout, wgpu::Sampler, wgpu::RenderPipeline) {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra-web-frame-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("astra-web-frame-shader"),
            source: wgpu::ShaderSource::Wgsl(FRAME_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("astra-web-frame-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("astra-web-frame-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        (layout, sampler, pipeline)
    }

    fn web_error(operation: &'static str) -> PlatformError {
        PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            operation,
            "browser platform operation failed",
        )
    }

    const FRAME_SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0)
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(frame_texture));
    return textureSample(frame_texture, frame_sampler, position.xy / dimensions);
}
"#;
}
