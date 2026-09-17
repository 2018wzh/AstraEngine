use super::*;
const MAX_WSCROLL2_SYNC_BYTES: u64 = 64 * 1024;
const MAX_WSCROLL2_SYNC_VALUES: usize = 4096;

impl Scene {
    pub(super) fn wscroll2_stage(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        state: &MusicaRuntimeState,
    ) -> FamilyResult<()> {
        let scroll = state
            .wscroll2
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_WSCROLL2_STATE", "missing WScroll2 state"))?;
        if (self.width, self.height) != (1280, 720) {
            return Err(error(
                "ASTRA_EMU_MUSICA_WSCROLL2_STAGE_IDENTITY",
                "WScroll2 requires a 1280x720 stage",
            ));
        }
        if !self.wscroll2_sync.contains(&scroll.sync_resource_uri) {
            let bytes = read_asset(
                &self.archive,
                &scroll.sync_resource_uri,
                MAX_WSCROLL2_SYNC_BYTES,
            )?;
            let values = parse_wscroll2_sync(&bytes)?;
            self.wscroll2_sync
                .put(scroll.sync_resource_uri.clone(), values);
        }
        let stage = state
            .stage
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_WSCROLL2_STATE", "missing WScroll2 stage"))?;
        let mut layers = Vec::new();
        self.stage(&mut layers, stage)?;
        layers.retain(|command| {
            !matches!(
                command,
                SceneCommand::PushClip { .. } | SceneCommand::PopClip
            )
        });
        if layers.len() != 2 {
            return Err(error(
                "ASTRA_EMU_MUSICA_WSCROLL2_STAGE_LAYERS",
                "WScroll2 requires one far and one near panorama",
            ));
        }
        commands.push(SceneCommand::PushClip {
            rect: RectI {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
        });
        for (index, layer) in layers.into_iter().enumerate() {
            let (frame, opacity, blend) = match layer {
                SceneCommand::Texture {
                    frame,
                    opacity,
                    blend,
                    ..
                } => (frame, opacity, blend),
                _ => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_WSCROLL2_STAGE_LAYERS",
                        "unsupported panorama draw",
                    ))
                }
            };
            if frame.width < self.width || frame.height < self.height {
                return Err(error(
                    "ASTRA_EMU_MUSICA_WSCROLL2_PANORAMA_BOUNDS",
                    "panorama is smaller than the viewport",
                ));
            }
            let offset = if index == 0 {
                scroll.background_offset
            } else {
                scroll.foreground_offset
            };
            let source_x = offset.rem_euclid(i64::from(frame.width)) as i32;
            for x in [-source_x, frame.width as i32 - source_x] {
                if x >= self.width as i32 {
                    continue;
                }
                commands.push(SceneCommand::Texture {
                    id: format!("wscroll2:{index}:{x}"),
                    destination: RectI {
                        x,
                        y: 0,
                        width: frame.width,
                        height: frame.height,
                    },
                    frame: frame.clone(),
                    opacity,
                    blend,
                });
            }
        }
        commands.push(SceneCommand::PopClip);
        Ok(())
    }
}
fn parse_wscroll2_sync(bytes: &[u8]) -> FamilyResult<Vec<i32>> {
    let source = std::str::from_utf8(bytes).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_WSCROLL2_SYNC_ENCODING",
            "WScroll2 sync resource is not bounded ASCII text",
        )
    })?;
    let mut values = Vec::new();
    for line in source.lines() {
        let token = line.trim();
        if token.is_empty() || token.starts_with(';') {
            continue;
        }
        if values.len() >= MAX_WSCROLL2_SYNC_VALUES {
            return Err(error(
                "ASTRA_EMU_MUSICA_WSCROLL2_SYNC_BOUNDS",
                "WScroll2 sync resource exceeds the value limit",
            ));
        }
        let value = token.parse::<i32>().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync resource contains a non-integer row",
            )
        })?;
        if !(-16_384..=16_384).contains(&value) {
            return Err(error(
                "ASTRA_EMU_MUSICA_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync value exceeds the verified bound",
            ));
        }
        values.push(value);
    }
    if values.is_empty() {
        return Err(error(
            "ASTRA_EMU_MUSICA_WSCROLL2_SYNC_EMPTY",
            "WScroll2 sync resource contains no values",
        ));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mount_musica, parse_sc, MusicaVm, ScOpcodeCatalog, MUSICA_PROFILE_FILE};
    use astra_core::Hash256;
    #[test]
    fn wscroll2_sync_rejects_invalid_and_excessive_rows() {
        assert_eq!(
            parse_wscroll2_sync(b"; comment\r\n13\r\n16\r\n").unwrap(),
            [13, 16]
        );
        for bytes in [b"not-a-number".as_slice(), b"", b"16385", b"\xff"] {
            assert!(parse_wscroll2_sync(bytes).is_err());
        }
        assert!(parse_wscroll2_sync("1\n".repeat(4097).as_bytes()).is_err());
    }
    #[test]
    #[ignore = "requires a hardware GPU"]
    fn wscroll2_gpu_wraps_both_layers_and_restores() {
        let root = tempfile::tempdir().unwrap();
        let source =
            b".stage BG.png BG.png 0 0\r\n.effect WScroll2 sync:walk.txt 60 -8\r\n.effect end\r\n";
        crate::test_fixture::game(root.path(), source);
        let color = |x: u32| {
            [
                ((x % 251) + 1) as u8,
                30,
                90,
                if x % 4 < 2 { 255 } else { 0 },
            ]
        };
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_fn(1280, 720, |x, _| image::Rgba(color(x)))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        crate::test_fixture::asset(root.path(), "bg", "BG.png", png.get_ref());
        crate::test_fixture::asset(root.path(), "st", "walk.txt", b"13\r\n16\r\n");
        let archive =
            Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
        let make_vm = || {
            MusicaVm::new(
                "musica:/scr/test.sc".into(),
                Hash256::from_sha256(source),
                parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
                1,
            )
            .unwrap()
        };
        let mut vm = make_vm();
        vm.step(1).unwrap();
        vm.step(2).unwrap();
        vm.advance_wscroll2_clock(166_666_667).unwrap();
        let mut scene = Scene::new(archive.clone(), 1280, 720).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        for x in 0..1280u32 {
            let near = color((x + 1280 - 8) % 1280);
            let far = color((x + 1280 - 1) % 1280);
            let expected = if near[3] != 0 {
                near
            } else if far[3] != 0 {
                far
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(
                &scene.pixels[x as usize * 4..x as usize * 4 + 4],
                &expected,
                "x={x}"
            );
        }
        let expected = scene.pixels.clone();
        let mut restored = make_vm();
        restored
            .restore_native_save(&vm.encode_native_save().unwrap(), 3)
            .unwrap();
        let mut restored_scene = Scene::new(archive, 1280, 720).unwrap();
        restored_scene.render(restored.state(), None, None).unwrap();
        assert_eq!(restored_scene.pixels, expected);
        vm.step(3).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        assert_ne!(scene.pixels, expected);
    }
}
