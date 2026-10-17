use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) struct Captures {
    pending: BTreeSet<u32>,
    output: PathBuf,
}

impl Captures {
    pub fn new(frames: &[u32], total: u32, output: &Path) -> Result<Self, String> {
        let pending: BTreeSet<_> = frames.iter().copied().collect();
        if frames.len() > 128 || pending.len() != frames.len() || frames.iter().any(|f| *f >= total)
        {
            return Err("ASTRA_EMU_HEADLESS_CAPTURE_FRAMES".into());
        }
        if output.file_name().is_none() {
            return Err("ASTRA_EMU_HEADLESS_CAPTURE_PATH".into());
        }
        Ok(Self {
            pending,
            output: output.to_owned(),
        })
    }

    pub fn write(
        &mut self,
        frame: u32,
        capture: impl FnOnce() -> Result<image::RgbaImage, String>,
    ) -> Result<(), String> {
        if !self.pending.contains(&frame) {
            return Ok(());
        }
        let mut name = self
            .output
            .file_name()
            .expect("validated output filename")
            .to_os_string();
        name.push(format!(".frame-{frame}.png"));
        capture()?
            .save_with_format(self.output.with_file_name(name), image::ImageFormat::Png)
            .map_err(|_| "ASTRA_EMU_HEADLESS_CAPTURE_WRITE")?;
        self.pending.remove(&frame);
        tracing::info!(event = "astra.emu.headless.capture.written", frame);
        Ok(())
    }

    pub fn finish(&self) -> Result<(), String> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("ASTRA_EMU_HEADLESS_CAPTURE_NOT_REACHED".into())
        }
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
}
