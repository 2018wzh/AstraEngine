use rfvp::host_api::{
    ColorRgba, DrawSolidCommand, DrawSpriteCommand, PixelFormat, RfvpError, RfvpRenderer,
    RfvpResult, TextureDesc, TextureId, TextureRect,
};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq)]
enum FrameCommand {
    Begin {
        width: u32,
        height: u32,
        clear: Option<ColorRgba>,
    },
    Sprite(DrawSpriteCommand),
    Solid(DrawSolidCommand),
    End,
    Present,
}

/// Observes the native renderer's existing metadata pass. It never uploads,
/// retains, hashes or rasterizes pixels; the original GPU renderer does that.
pub(super) struct SceneTracker {
    max_commands: usize,
    textures: BTreeMap<u32, TextureDesc>,
    current: Vec<FrameCommand>,
    previous: Vec<FrameCommand>,
    resources_changed: bool,
    changed: bool,
    failure: Option<RfvpError>,
}
impl SceneTracker {
    pub(super) fn new(max_commands: usize) -> Self {
        Self {
            max_commands,
            textures: BTreeMap::new(),
            current: Vec::new(),
            previous: Vec::new(),
            resources_changed: true,
            changed: true,
            failure: None,
        }
    }
    pub(super) fn reset(&mut self) {
        self.textures.clear();
        self.current.clear();
        self.previous.clear();
        self.resources_changed = true;
        self.changed = true;
        self.failure = None;
    }
    pub(super) fn finish(&self) -> RfvpResult<()> {
        self.failure.map_or(Ok(()), Err)
    }
    pub(super) fn has_frame(&self) -> bool {
        !self.current.is_empty() || !self.previous.is_empty()
    }
    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn push(&mut self, command: FrameCommand) -> RfvpResult<()> {
        self.finish()?;
        if self.current.len() >= self.max_commands {
            return Err(RfvpError::CapacityExceeded);
        }
        self.current.push(command);
        Ok(())
    }
}
impl RfvpRenderer for SceneTracker {
    fn create_texture(
        &mut self,
        id: TextureId,
        desc: TextureDesc,
        pixels: Option<&[u8]>,
    ) -> RfvpResult<()> {
        self.finish()?;
        if desc.width == 0 || desc.height == 0 || desc.mip_count == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        if let Some(pixels) = pixels {
            validate_pixels(desc, pixels.len())?;
        }
        if self.textures.len() >= self.max_commands && !self.textures.contains_key(&id.0) {
            return Err(RfvpError::CapacityExceeded);
        }
        self.textures.insert(id.0, desc);
        self.resources_changed = true;
        Ok(())
    }
    fn update_texture(
        &mut self,
        id: TextureId,
        rect: TextureRect,
        pixels: &[u8],
    ) -> RfvpResult<()> {
        self.finish()?;
        let desc = self.textures.get(&id.0).ok_or(RfvpError::NotFound)?;
        if rect.width == 0
            || rect.height == 0
            || rect
                .x
                .checked_add(rect.width)
                .ok_or(RfvpError::CapacityExceeded)?
                > desc.width
            || rect
                .y
                .checked_add(rect.height)
                .ok_or(RfvpError::CapacityExceeded)?
                > desc.height
        {
            return Err(RfvpError::InvalidArgument);
        }
        validate_pixels(
            TextureDesc {
                width: rect.width,
                height: rect.height,
                format: desc.format,
                mip_count: 1,
            },
            pixels.len(),
        )?;
        self.resources_changed = true;
        Ok(())
    }
    fn destroy_texture(&mut self, id: TextureId) {
        if self.failure.is_some() {
            return;
        }
        if self.textures.remove(&id.0).is_none() {
            self.failure = Some(RfvpError::NotFound);
            return;
        }
        self.resources_changed = true;
    }
    fn begin_frame(&mut self, width: u32, height: u32, clear: Option<ColorRgba>) -> RfvpResult<()> {
        if width == 0 || height == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        self.push(FrameCommand::Begin {
            width,
            height,
            clear,
        })
    }
    fn draw_sprite(&mut self, command: &DrawSpriteCommand) -> RfvpResult<()> {
        self.push(FrameCommand::Sprite(*command))
    }
    fn draw_solid(&mut self, command: &DrawSolidCommand) -> RfvpResult<()> {
        self.push(FrameCommand::Solid(*command))
    }
    fn end_frame(&mut self) -> RfvpResult<()> {
        self.push(FrameCommand::End)
    }
    fn present(&mut self) -> RfvpResult<()> {
        self.push(FrameCommand::Present)?;
        self.changed = self.resources_changed || self.current != self.previous;
        std::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear();
        self.resources_changed = false;
        Ok(())
    }
}
fn validate_pixels(desc: TextureDesc, bytes: usize) -> RfvpResult<()> {
    let channels = match desc.format {
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
        PixelFormat::Rgb8 => 3,
        PixelFormat::LumaA8 => 2,
        PixelFormat::Luma8 | PixelFormat::Alpha8 => 1,
    };
    let expected = (desc.width as usize)
        .checked_mul(desc.height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or(RfvpError::CapacityExceeded)?;
    if bytes != expected {
        return Err(RfvpError::InvalidData);
    }
    Ok(())
}
