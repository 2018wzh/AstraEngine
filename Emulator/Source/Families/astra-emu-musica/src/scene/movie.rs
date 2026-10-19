use super::*;
use astra_emu_sdk::{video::VideoFramePacket, AssScene};

const MOVIE_TEXTURES: [&str; 2] = ["musica.movie.retained.0", "musica.movie.retained.1"];
#[derive(Default)]
pub(super) struct MovieScene {
    catalog: Option<Arc<crate::subtitles::SubtitleCatalog>>,
    track: Option<AssScene>,
    resource: Option<String>,
    generation: Option<u64>,
    texture_loaded: bool,
    texture_slot: usize,
    track_serial: u64,
    pending: Vec<SceneCommand>,
}
impl Scene {
    pub(crate) fn set_subtitle_catalog(
        &mut self,
        catalog: Option<Arc<crate::subtitles::SubtitleCatalog>>,
    ) {
        self.movies.catalog = catalog;
    }
    pub(crate) fn subtitle_catalog(&self) -> Option<Arc<crate::subtitles::SubtitleCatalog>> {
        self.movies.catalog.clone()
    }
    pub(crate) fn begin_movie(&mut self, resource: &str) -> FamilyResult<()> {
        self.end_movie();
        self.movies.track_serial = self.movies.track_serial.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_MOVIE_STATE",
                "movie track serial overflowed",
            )
        })?;
        self.movies.track = self
            .movies
            .catalog
            .as_ref()
            .map(|catalog| catalog.open_movie(resource, self.movies.track_serial))
            .transpose()
            .map_err(core_error)?
            .flatten();
        self.movies.resource = Some(resource.to_owned());
        Ok(())
    }
    pub(crate) fn end_movie(&mut self) {
        if let Some(mut track) = self.movies.track.take() {
            track.release_resources(&mut self.movies.pending);
        }
        if self.movies.texture_loaded {
            self.movies.pending.push(SceneCommand::ReleaseResource {
                resource_id: MOVIE_TEXTURES[self.movies.texture_slot].into(),
            });
        }
        self.movies.texture_loaded = false;
        self.movies.resource = None;
        self.movies.generation = None;
    }
    pub(super) fn append_movie_mutations(&mut self, commands: &mut Vec<SceneCommand>) {
        if !self.movies.pending.is_empty() {
            self.movies.pending.append(commands);
            std::mem::swap(commands, &mut self.movies.pending);
        }
    }
    pub(crate) fn render_movie_with_subtitles(
        &mut self,
        frame: Option<TextureFrame>,
        position_us: u64,
        packet: Option<&VideoFramePacket>,
    ) -> FamilyResult<()> {
        let Some(packet) = packet else {
            return Ok(());
        };
        if self
            .movies
            .generation
            .is_some_and(|generation| generation != packet.generation)
        {
            let resource =
                self.movies.resource.clone().ok_or_else(|| {
                    error("ASTRA_EMU_MUSICA_MOVIE_STATE", "movie resource is absent")
                })?;
            self.begin_movie(&resource)?;
        }
        self.movies.generation = Some(packet.generation);
        let mut commands = Vec::new();
        let mut changed = frame.is_some();
        if let Some(frame) = frame {
            if self.movies.texture_loaded {
                commands.push(SceneCommand::ReleaseResource {
                    resource_id: MOVIE_TEXTURES[self.movies.texture_slot].into(),
                });
            }
            self.movies.texture_slot ^= 1;
            commands.push(SceneCommand::UploadTexture {
                resource_id: MOVIE_TEXTURES[self.movies.texture_slot].into(),
                frame,
            });
            self.movies.texture_loaded = true;
        }
        if !self.movies.texture_loaded {
            return Ok(());
        }
        if let Some(track) = &mut self.movies.track {
            let viewport = self.canvas.viewport_rect().map_err(core_error)?;
            changed |= track
                .update(
                    position_us,
                    (packet.width, packet.height),
                    viewport,
                    crate::subtitles::video_color(packet.color).map_err(core_error)?,
                    &mut commands,
                )
                .map_err(core_error)?;
        }
        if !changed {
            return Ok(());
        }
        commands.push(SceneCommand::Sprite {
            id: "musica.movie".into(),
            texture_id: MOVIE_TEXTURES[self.movies.texture_slot].into(),
            source: None,
            destination: RectI {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
        if let Some(track) = &self.movies.track {
            // ASS is rasterized at the actual video viewport, not the logical game extent.
            commands.push(SceneCommand::PushTransform {
                transform: self.canvas.raster_to_logical_transform(),
            });
            track.append_draws(&mut commands);
            commands.push(SceneCommand::PopTransform);
        }
        self.submit(commands)
    }
}
