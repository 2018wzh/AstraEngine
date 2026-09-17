use crate::{audio::Audio, scene::error, MusicaMountedVfs, MusicaMovieState};
use astra_emu_family_api::FamilyResult;
use astra_media_core::TextureFrame;

#[cfg(feature = "ffmpeg-vcpkg")]
mod playback;
#[cfg(feature = "ffmpeg-vcpkg")]
pub(crate) use playback::Movie;

#[cfg(not(feature = "ffmpeg-vcpkg"))]
pub(crate) struct Movie;
#[cfg(not(feature = "ffmpeg-vcpkg"))]
impl Movie {
    pub fn open(_: &MusicaMountedVfs, _: &MusicaMovieState) -> FamilyResult<Self> {
        Err(error(
            "ASTRA_EMU_MUSICA_MOVIE_UNAVAILABLE",
            "movie playback requires ffmpeg-vcpkg",
        ))
    }
    pub fn advance(&mut self, _: &Audio, _: u64, _: bool) -> FamilyResult<Option<TextureFrame>> {
        unreachable!()
    }
    pub fn position_us(&self) -> u64 {
        unreachable!()
    }
    pub fn ended(&self) -> bool {
        unreachable!()
    }
    pub fn close(self, _: &Audio) -> FamilyResult<()> {
        unreachable!()
    }
}
