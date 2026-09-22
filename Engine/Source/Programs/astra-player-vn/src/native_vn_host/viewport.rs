use super::*;
use astra_media_core::{Canvas2D, Extent2D};

impl NativeVnHostCommandSource {
    fn output_canvas(&self) -> Result<Canvas2D, NativeVnHostError> {
        Ok(Canvas2D::new(
            Extent2D {
                width: self.width,
                height: self.height,
            },
            self.output_extent,
        )?)
    }

    // Window dimensions belong to presentation, never to authored stage coordinates or saves.
    pub(super) fn map_window_input(
        &mut self,
        events: Vec<UiInputEvent>,
    ) -> Result<Vec<UiInputEvent>, NativeVnHostError> {
        let mut mapped = Vec::with_capacity(events.len());
        for mut event in events {
            match &mut event.kind {
                UiInputEventKind::Resize { viewport } => {
                    viewport.validate()?;
                    self.output_extent = Extent2D {
                        width: viewport.physical_width,
                        height: viewport.physical_height,
                    };
                    let canvas = self.output_canvas()?;
                    let fit = canvas.viewport();
                    let scale = canvas.scale_x();
                    let device_scale = viewport.scale_factor;
                    viewport.safe_area_points.left =
                        ((viewport.safe_area_points.left * device_scale - fit.x as f32) / scale)
                            .max(0.0);
                    viewport.safe_area_points.top =
                        ((viewport.safe_area_points.top * device_scale - fit.y as f32) / scale)
                            .max(0.0);
                    viewport.safe_area_points.right = ((viewport.safe_area_points.right
                        * device_scale
                        - (self.output_extent.width - fit.x - fit.width) as f32)
                        / scale)
                        .max(0.0);
                    viewport.safe_area_points.bottom = ((viewport.safe_area_points.bottom
                        * device_scale
                        - (self.output_extent.height - fit.y - fit.height) as f32)
                        / scale)
                        .max(0.0);
                    viewport.physical_width = self.width;
                    viewport.physical_height = self.height;
                    viewport.scale_factor = 1.0;
                    self.ui_viewport = viewport.clone();
                    self.ui_frame_reuse = None;
                }
                UiInputEventKind::PointerMove { position }
                | UiInputEventKind::PointerButton { position, .. }
                | UiInputEventKind::Touch { position, .. } => {
                    let canvas = self.output_canvas()?;
                    if !canvas.contains_raster_point([position.x, position.y]) {
                        continue;
                    }
                    let point = canvas.raster_to_logical_point([position.x, position.y])?;
                    position.x = point[0];
                    position.y = point[1];
                }
                _ => {}
            }
            mapped.push(event);
        }
        Ok(mapped)
    }

    pub(super) fn map_output_scene(
        &self,
        mut commands: Vec<SceneCommand>,
    ) -> Result<Vec<SceneCommand>, NativeVnHostError> {
        let canvas = self.output_canvas()?;
        commands.insert(
            0,
            SceneCommand::PushTransform {
                transform: canvas.logical_to_raster_transform(),
            },
        );
        commands.insert(
            0,
            SceneCommand::PushClip {
                rect: canvas.viewport_rect()?,
            },
        );
        commands.push(SceneCommand::PopTransform);
        commands.push(SceneCommand::PopClip);
        Ok(commands)
    }

    pub(super) fn output_semantics(&self) -> Result<Option<UiSemanticSnapshot>, NativeVnHostError> {
        let canvas = self.output_canvas()?;
        let mut snapshot = self.ui_semantics.clone();
        if let Some(snapshot) = &mut snapshot {
            for node in &mut snapshot.nodes {
                for point in [&mut node.bounds_points.min, &mut node.bounds_points.max] {
                    let mapped = canvas.logical_to_raster_point([point.x, point.y])?;
                    point.x = mapped[0];
                    point.y = mapped[1];
                }
            }
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_ui_core::{UiInsets, UiPoint};

    #[test]
    fn resize_keeps_authored_stage_and_maps_letterboxed_input_and_gpu_commands() {
        let bytes = crate::test_native_package::product_package_with_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n", |_| {});
        let package = astra_package::PackageReader::open(&bytes).unwrap();
        let mut source = NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig::classic("en"),
            320,
            180,
            PlayerHostResourceId(1),
        )
        .unwrap();
        source.launch().unwrap();
        let stage = source.stage_director.snapshot().unwrap();
        for (width, height) in [(1001, 777), (180, 320), (320, 180)] {
            let resize = source
                .next_ui_event(UiInputEventKind::Resize {
                    viewport: UiViewport {
                        physical_width: width,
                        physical_height: height,
                        scale_factor: 1.0,
                        font_scale: 1.0,
                        safe_area_points: UiInsets {
                            left: 0.0,
                            top: 0.0,
                            right: 0.0,
                            bottom: 0.0,
                        },
                    },
                })
                .unwrap();
            source.dispatch_ui_events(vec![resize]).unwrap();
            assert_eq!(source.stage_director.snapshot().unwrap(), stage);
            let canvas = source.output_canvas().unwrap();
            let point = canvas.logical_to_raster_point([160.0, 90.0]).unwrap();
            let input = source
                .next_ui_event(UiInputEventKind::PointerMove {
                    position: UiPoint {
                        x: point[0],
                        y: point[1],
                    },
                })
                .unwrap();
            let mapped = source.map_window_input(vec![input]).unwrap();
            let UiInputEventKind::PointerMove { position } = mapped[0].kind else {
                panic!("pointer")
            };
            assert!((position.x - 160.0).abs() < 0.001 && (position.y - 90.0).abs() < 0.001);
            if width != 320 {
                let bar = source
                    .next_ui_event(UiInputEventKind::PointerMove {
                        position: UiPoint { x: 0.0, y: 0.0 },
                    })
                    .unwrap();
                assert!(source.map_window_input(vec![bar]).unwrap().is_empty());
            }
            let batch = source.present_current_scene(Vec::new()).unwrap();
            let PlayerHostCommand::PresentScene {
                width: output_width,
                height: output_height,
                commands,
                ..
            } = &batch.commands[0]
            else {
                panic!("scene")
            };
            assert_eq!((*output_width, *output_height), (width, height));
            assert!(
                matches!(commands.first(), Some(SceneCommand::PushClip { rect }) if *rect == canvas.viewport_rect().unwrap())
            );
            assert!(
                matches!(&commands[1], SceneCommand::PushTransform { transform } if *transform == canvas.logical_to_raster_transform())
            );
        }
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}
