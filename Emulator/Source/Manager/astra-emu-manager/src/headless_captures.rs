use std::{collections::BTreeSet, path::{Path, PathBuf}};

pub(super) struct Captures {
    pending: BTreeSet<u32>,
    output: PathBuf,
}

impl Captures {
    pub fn new(frames: &[u32], total: u32, output: &Path) -> Result<Self, String> {
        let pending: BTreeSet<_> = frames.iter().copied().collect();
        if frames.len() > 128 || pending.len() != frames.len() || frames.iter().any(|f| *f >= total) {
            return Err("ASTRA_EMU_HEADLESS_CAPTURE_FRAMES".into());
        }
        if output.file_name().is_none() {
            return Err("ASTRA_EMU_HEADLESS_CAPTURE_PATH".into());
        }
        Ok(Self { pending, output: output.to_owned() })
    }

    pub fn write(&mut self, frame: u32, image: Option<&image::RgbaImage>) -> Result<(), String> {
        if !self.pending.contains(&frame) {
            return Ok(());
        }
        let mut name = self.output.file_name().expect("validated output filename").to_os_string();
        name.push(format!(".frame-{frame}.png"));
        image.ok_or("ASTRA_EMU_HEADLESS_FRAME_MISSING")?
            .save_with_format(self.output.with_file_name(name), image::ImageFormat::Png)
            .map_err(|_| "ASTRA_EMU_HEADLESS_CAPTURE_WRITE")?;
        self.pending.remove(&frame);
        tracing::info!(event = "astra.emu.headless.capture.written", frame);
        Ok(())
    }

    pub fn finish(&self) -> Result<(), String> {
        if self.pending.is_empty() { Ok(()) } else { Err("ASTRA_EMU_HEADLESS_CAPTURE_NOT_REACHED".into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_out_of_range_and_excessive_requests() {
        let output = Path::new("final.png");
        assert!(Captures::new(&[1, 1], 3, output).is_err());
        assert!(Captures::new(&[3], 3, output).is_err());
        assert!(Captures::new(&(0..129).collect::<Vec<_>>(), 200, output).is_err());
    }

    #[test]
    fn saves_requested_pixels_and_rejects_unreached_frames() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("final.png");
        let mut captures = Captures::new(&[2, 4], 5, &output).unwrap();
        let pixels = image::RgbaImage::from_pixel(2, 3, image::Rgba([12, 34, 56, 255]));
        captures.write(1, None).unwrap();
        assert!(captures.write(2, None).is_err());
        assert!(captures.finish().is_err());
        captures.write(2, Some(&pixels)).unwrap();
        assert_eq!(image::open(directory.path().join("final.png.frame-2.png")).unwrap().to_rgba8(), pixels);
        assert!(captures.finish().is_err());
        captures.write(4, Some(&pixels)).unwrap();
        captures.finish().unwrap();
        assert!(!output.exists());
    }
}
