use super::*;
use crate::{audio::MoviePcm, scene::core_error};
use astra_emu_sdk::video::*;
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, Arc},
};

pub(crate) struct Movie {
    decoder: VideoDecoderWorker,
    audio: Option<Arc<MoviePcm>>,
    frames: VecDeque<(VideoFramePacket, astra_byte_source::OwnedByteBuffer)>,
    video_bytes: usize,
    position: u64,
    duration: u64,
    has_audio: bool,
    initialized: bool,
    pending: bool,
    eof: bool,
    remainder_ns: u64,
}
impl Movie {
    pub fn open(archive: &MusicaMountedVfs, state: &MusicaMovieState) -> FamilyResult<Self> {
        let stat = archive.stat(&state.resource_uri).map_err(core_error)?;
        if stat.size == 0 || stat.size > 512 * 1024 * 1024 {
            return Err(error(
                "ASTRA_EMU_MUSICA_MOVIE_RESOURCE",
                "movie exceeds encoded byte budget",
            ));
        }
        let bytes = archive
            .read_range(&state.resource_uri, 0, stat.size)
            .map_err(core_error)?
            .bytes;
        let codec = state
            .resource_uri
            .rsplit_once('.')
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_MOVIE_RESOURCE",
                    "movie has no codec extension",
                )
            })?
            .1
            .to_owned();
        let decoder = VideoDecoderWorker::open(
            codec,
            Arc::from(bytes.as_ref()),
            FfmpegStreamLimits {
                max_encoded_bytes: 512 * 1024 * 1024,
                max_audio_packet_bytes: 64 * 1024,
                ..Default::default()
            },
            Some(FfmpegAudioOutputFormat {
                sample_rate: 48000,
                channels: 2,
            }),
        )
        .map_err(core_error)?;
        tracing::info!(
            event = "astra.emu.musica.movie.open",
            position_us = state.continuation_pts
        );
        Ok(Self {
            decoder,
            audio: None,
            frames: VecDeque::new(),
            video_bytes: 0,
            position: state.continuation_pts,
            duration: 0,
            has_audio: false,
            initialized: false,
            pending: true,
            eof: false,
            remainder_ns: 0,
        })
    }
    fn start_audio(&mut self, host: &Audio, generation: u64) -> FamilyResult<()> {
        if self.has_audio {
            let audio = Arc::new(MoviePcm::new(generation, self.position)?);
            host.set_movie(Some(audio.clone()))?;
            self.audio = Some(audio);
        }
        self.initialized = true;
        Ok(())
    }
    pub fn advance(
        &mut self,
        host: &Audio,
        elapsed_ns: u64,
        paused: bool,
    ) -> FamilyResult<Option<TextureFrame>> {
        if let Some(audio) = &self.audio {
            audio.paused.store(paused, Ordering::Release);
        }
        if self.pending {
            if let Some(result) = self.decoder.poll().map_err(core_error)? {
                self.pending = false;
                match result {
                    DecodeCompletion::Opened(config) => {
                        if !config.has_video || self.position >= config.duration_us {
                            return Err(error(
                                "ASTRA_EMU_MUSICA_MOVIE_FORMAT",
                                "movie has no video or invalid saved position",
                            ));
                        }
                        self.duration = config.duration_us;
                        self.has_audio = config.has_audio;
                        if self.position != 0 {
                            self.decoder
                                .request_seek(self.position)
                                .map_err(core_error)?;
                            self.pending = true;
                        } else {
                            self.start_audio(host, 1)?;
                        }
                    }
                    DecodeCompletion::Seeked { generation } => {
                        self.start_audio(host, generation)?
                    }
                    DecodeCompletion::Packet(None) => self.eof = true,
                    DecodeCompletion::Packet(Some(DecodedMediaPacket::Audio {
                        packet,
                        samples,
                    })) => {
                        self.audio
                            .as_ref()
                            .ok_or_else(|| {
                                error("ASTRA_EMU_MUSICA_MOVIE_FORMAT", "unexpected audio packet")
                            })?
                            .push(packet, samples)?;
                    }
                    DecodeCompletion::Packet(Some(DecodedMediaPacket::Video { packet, bgra8 })) => {
                        self.video_bytes += bgra8.len();
                        self.frames.push_back((packet, bgra8));
                    }
                }
            }
        }
        if self.initialized
            && !self.pending
            && !self.eof
            && self.frames.len() < 16
            && self.video_bytes < 64 * 1024 * 1024
            && self
                .audio
                .as_ref()
                .map(|a| a.buffered_frames())
                .transpose()?
                .unwrap_or(0)
                < 48000
        {
            self.decoder.request_next().map_err(core_error)?;
            self.pending = true;
        }
        let audio_drained = self
            .audio
            .as_ref()
            .map(|a| a.drained())
            .transpose()?
            .unwrap_or(true);
        if self.initialized && !paused {
            if let Some(audio) = &self.audio {
                self.position = self.position.max(audio.position_us());
            }
            // Silent movies, and the video tail after audio EOS, use the host clock.
            if (!self.has_audio && (!self.frames.is_empty() || self.eof))
                || (self.eof && audio_drained)
            {
                let ns = self.remainder_ns + elapsed_ns;
                self.position = self.position.saturating_add(ns / 1000).min(self.duration);
                self.remainder_ns = ns % 1000;
            }
        }
        if !paused
            && self
                .frames
                .front()
                .is_some_and(|(p, _)| p.pts_us <= self.position)
        {
            let (packet, bytes) = self.frames.pop_front().unwrap();
            self.video_bytes -= bytes.len();
            let mut rgba = bytes.as_ref().to_vec();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            return Ok(Some(TextureFrame {
                width: packet.width,
                height: packet.height,
                rgba8: rgba.into(),
            }));
        }
        Ok(None)
    }
    pub fn position_us(&self) -> u64 {
        self.position
    }
    pub fn ended(&self) -> bool {
        self.eof && self.frames.is_empty() && self.position >= self.duration
    }
    pub fn close(self, host: &Audio) -> FamilyResult<()> {
        let audio = host.set_movie(None);
        let decoder = self.decoder.close().map_err(core_error);
        audio?;
        decoder?;
        tracing::info!(event = "astra.emu.musica.movie.close");
        Ok(())
    }
}
