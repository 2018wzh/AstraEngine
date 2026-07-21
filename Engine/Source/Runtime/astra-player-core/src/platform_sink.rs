use std::{collections::BTreeMap, future::Future, pin::Pin};

use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioPacket, DecodeKind, DecodeOutput,
    DecodeSessionHandle, PackageSourceHandle, PackageSourceRequest, PlatformDecodeRequest,
    PlatformError, PlatformErrorCode, PlatformHostClient, SaveTransactionHandle, SurfaceHandle,
};

use crate::{
    PlayerDecodeKind, PlayerHostCommand, PlayerHostCommandResult, PlayerHostCommandSink,
    PlayerHostResourceId, PlayerPackageSource,
};

pub struct PlatformCommandSink {
    client: PlatformHostClient,
    packages: BTreeMap<PlayerHostResourceId, PackageSourceHandle>,
    saves: BTreeMap<PlayerHostResourceId, SaveTransactionHandle>,
    audio: BTreeMap<PlayerHostResourceId, AudioOutputHandle>,
    decoders: BTreeMap<PlayerHostResourceId, DecodeSessionHandle>,
    surfaces: BTreeMap<PlayerHostResourceId, SurfaceHandle>,
}

impl PlatformCommandSink {
    pub fn new(client: PlatformHostClient) -> Self {
        Self {
            client,
            packages: BTreeMap::new(),
            saves: BTreeMap::new(),
            audio: BTreeMap::new(),
            decoders: BTreeMap::new(),
            surfaces: BTreeMap::new(),
        }
    }

    pub fn bind_surface(
        &mut self,
        logical: PlayerHostResourceId,
        surface: SurfaceHandle,
    ) -> Result<(), PlatformError> {
        insert_unique(&mut self.surfaces, logical, surface, "surface.bind")
    }

    pub fn has_live_resources(&self) -> bool {
        !(self.packages.is_empty()
            && self.saves.is_empty()
            && self.audio.is_empty()
            && self.decoders.is_empty())
    }
}

impl PlayerHostCommandSink for PlatformCommandSink {
    type Error = PlatformError;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>> {
        Box::pin(async move { self.execute_platform(command).await })
    }
}

impl PlatformCommandSink {
    async fn execute_platform(
        &mut self,
        command: &PlayerHostCommand,
    ) -> Result<PlayerHostCommandResult, PlatformError> {
        match command {
            PlayerHostCommand::OpenPackage {
                source, package, ..
            } => {
                let handle = self.client.open_package(package_source(source)).await?;
                insert_unique(&mut self.packages, *package, handle, "package.open")?;
                Ok(PlayerHostCommandResult::PackageOpened { package: *package })
            }
            PlayerHostCommand::ReadPackageRange {
                package,
                offset,
                length,
                ..
            } => {
                let handle = lookup(&self.packages, package, "package.read_range")?;
                let bytes = self
                    .client
                    .read_package_range(handle, *offset, *length as usize)
                    .await?;
                Ok(PlayerHostCommandResult::PackageRange {
                    package: *package,
                    bytes,
                })
            }
            PlayerHostCommand::ClosePackage { package, .. } => {
                let handle = lookup(&self.packages, package, "package.close")?;
                self.client.close_package(handle).await?;
                self.packages.remove(package);
                Ok(PlayerHostCommandResult::PackageClosed { package: *package })
            }
            PlayerHostCommand::BeginSave {
                slot, transaction, ..
            } => {
                let handle = self.client.begin_save(slot.clone()).await?;
                insert_unique(&mut self.saves, *transaction, handle, "save.begin")?;
                Ok(PlayerHostCommandResult::SaveStarted {
                    transaction: *transaction,
                })
            }
            PlayerHostCommand::WriteSave {
                transaction, bytes, ..
            } => {
                let handle = lookup(&self.saves, transaction, "save.write")?;
                self.client.write_save(handle, bytes.clone()).await?;
                Ok(PlayerHostCommandResult::Unit)
            }
            PlayerHostCommand::CommitSave { transaction, .. } => {
                let handle = lookup(&self.saves, transaction, "save.commit")?;
                let hash = self.client.commit_save(handle).await?;
                self.saves.remove(transaction);
                Ok(PlayerHostCommandResult::SaveCommitted {
                    transaction: *transaction,
                    hash,
                })
            }
            PlayerHostCommand::AbortSave { transaction, .. } => {
                let handle = lookup(&self.saves, transaction, "save.abort")?;
                self.client.abort_save(handle).await?;
                self.saves.remove(transaction);
                Ok(PlayerHostCommandResult::Unit)
            }
            PlayerHostCommand::ReadSave { slot, .. } => Ok(PlayerHostCommandResult::SaveRead {
                bytes: self.client.read_save(slot.clone()).await?,
            }),
            PlayerHostCommand::ListSaves { .. } => Ok(PlayerHostCommandResult::SaveList {
                slots: self.client.list_saves().await?,
            }),
            PlayerHostCommand::DeleteSave { slot, .. } => {
                self.client.delete_save(slot.clone()).await?;
                Ok(PlayerHostCommandResult::Unit)
            }
            PlayerHostCommand::OpenAudio {
                output,
                sample_rate,
                channels,
                max_buffered_frames,
                ..
            } => {
                let handle = self
                    .client
                    .open_audio_output(AudioOutputRequest {
                        sample_rate: *sample_rate,
                        channels: *channels,
                        max_buffered_frames: *max_buffered_frames as usize,
                    })
                    .await?;
                insert_unique(&mut self.audio, *output, handle, "audio.open")?;
                Ok(PlayerHostCommandResult::AudioOpened { output: *output })
            }
            PlayerHostCommand::QueryAudioFormat { .. } => {
                let format = self.client.preferred_audio_output_format().await?;
                Ok(PlayerHostCommandResult::AudioFormat {
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                })
            }
            PlayerHostCommand::SubmitAudio {
                output,
                packet_sequence,
                channels,
                samples,
                ..
            } => {
                let handle = lookup(&self.audio, output, "audio.submit")?;
                self.client
                    .submit_audio(
                        handle,
                        AudioPacket {
                            sequence: *packet_sequence,
                            channels: *channels,
                            samples: samples.clone(),
                        },
                    )
                    .await?;
                Ok(PlayerHostCommandResult::Unit)
            }
            PlayerHostCommand::QueryAudio { output, .. } => {
                let handle = lookup(&self.audio, output, "audio.query")?;
                let state = self.client.query_audio(handle).await?;
                Ok(PlayerHostCommandResult::AudioState {
                    output: *output,
                    queued_frames: u64::try_from(state.queued_frames).map_err(|_| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::IntegrityMismatch,
                            "audio.query",
                            "queued frame count exceeds the player contract",
                        )
                    })?,
                    callback_count: state.callback_count,
                    submitted_samples: state.submitted_samples,
                    consumed_samples: state.consumed_samples,
                    underflow_count: state.underflow_count,
                    peak_dbfs_bits: state.meter.peak_dbfs.to_bits(),
                    rms_dbfs_bits: state.meter.rms_dbfs.to_bits(),
                })
            }
            PlayerHostCommand::DrainAudio { output, .. } => {
                let handle = lookup(&self.audio, output, "audio.drain")?;
                let meter = self.client.drain_audio(handle).await?;
                Ok(PlayerHostCommandResult::AudioDrained {
                    output: *output,
                    sample_count: meter.sample_count,
                    peak_dbfs_bits: meter.peak_dbfs.to_bits(),
                    rms_dbfs_bits: meter.rms_dbfs.to_bits(),
                })
            }
            PlayerHostCommand::CloseAudio { output, .. } => {
                let handle = lookup(&self.audio, output, "audio.close")?;
                self.client.close_audio(handle).await?;
                self.audio.remove(output);
                Ok(PlayerHostCommandResult::AudioClosed { output: *output })
            }
            PlayerHostCommand::OpenDecode { session, kind, .. } => {
                let handle = self.client.open_decode(decode_kind(*kind)).await?;
                insert_unique(&mut self.decoders, *session, handle, "decode.open")?;
                Ok(PlayerHostCommandResult::DecodeOpened { session: *session })
            }
            PlayerHostCommand::Decode {
                session,
                kind,
                codec,
                description,
                sample_rate,
                channels,
                coded_width,
                coded_height,
                keyframe,
                bytes,
                request_sequence,
                ..
            } => {
                let handle = lookup(&self.decoders, session, "decode.submit")?;
                match self
                    .client
                    .decode(
                        handle,
                        PlatformDecodeRequest {
                            sequence: *request_sequence,
                            kind: decode_kind(*kind),
                            codec: codec.clone(),
                            description: description.clone(),
                            sample_rate: *sample_rate,
                            channels: *channels,
                            coded_width: *coded_width,
                            coded_height: *coded_height,
                            keyframe: *keyframe,
                            bytes: bytes.clone(),
                        },
                    )
                    .await?
                {
                    DecodeOutput::CpuBuffer {
                        format,
                        bytes,
                        hash,
                    } => Ok(PlayerHostCommandResult::Decoded {
                        session: *session,
                        format,
                        hash,
                        bytes,
                    }),
                    DecodeOutput::MediaFrame(_) => Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "decode.submit",
                        "native media frames cannot cross the Player command boundary",
                    )),
                }
            }
            PlayerHostCommand::CloseDecode { session, .. } => {
                let handle = lookup(&self.decoders, session, "decode.close")?;
                self.client.close_decode(handle).await?;
                self.decoders.remove(session);
                Ok(PlayerHostCommandResult::DecodeClosed { session: *session })
            }
            PlayerHostCommand::PresentRgba {
                surface,
                sequence,
                width,
                height,
                rgba8,
            } => {
                let handle = lookup(&self.surfaces, surface, "surface.present_rgba")?;
                self.client
                    .present_rgba(
                        handle,
                        astra_platform::RgbaFrame {
                            sequence: *sequence,
                            width: *width,
                            height: *height,
                            rgba8: rgba8.clone(),
                        },
                    )
                    .await?;
                Ok(PlayerHostCommandResult::Presented { surface: *surface })
            }
            PlayerHostCommand::PresentScene {
                surface,
                sequence,
                width,
                height,
                clear_rgba,
                commands,
                semantics,
            } => {
                let handle = lookup(&self.surfaces, surface, "surface.present_scene")?;
                self.client
                    .present_scene(
                        handle,
                        astra_platform::SceneFrame {
                            sequence: *sequence,
                            width: *width,
                            height: *height,
                            clear_rgba: *clear_rgba,
                            commands: commands.clone(),
                            semantics: semantics.clone(),
                        },
                    )
                    .await?;
                Ok(PlayerHostCommandResult::Presented { surface: *surface })
            }
            PlayerHostCommand::CaptureSurface { surface, .. } => {
                let handle = lookup(&self.surfaces, surface, "surface.capture")?;
                let frame = self.client.capture_surface(handle).await?;
                Ok(PlayerHostCommandResult::Captured {
                    surface: *surface,
                    width: frame.width,
                    height: frame.height,
                    rgba8: frame.rgba8.to_vec(),
                })
            }
        }
    }
}

fn package_source(source: &PlayerPackageSource) -> PackageSourceRequest {
    match source {
        PlayerPackageSource::Bundled {
            relative_path,
            expected_hash,
        } => PackageSourceRequest::Bundled {
            relative_path: relative_path.clone(),
            expected_hash: expected_hash.clone(),
        },
        PlayerPackageSource::UserAuthorized { expected_hash } => {
            PackageSourceRequest::UserAuthorized {
                expected_hash: expected_hash.clone(),
            }
        }
        PlayerPackageSource::HttpsRange { url, expected_hash } => {
            PackageSourceRequest::HttpsRange {
                url: url.clone(),
                expected_hash: expected_hash.clone(),
            }
        }
    }
}

fn decode_kind(kind: PlayerDecodeKind) -> DecodeKind {
    match kind {
        PlayerDecodeKind::Audio => DecodeKind::Audio,
        PlayerDecodeKind::Video => DecodeKind::Video,
    }
}

fn insert_unique<K: Ord + Copy, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<(), PlatformError> {
    if map.contains_key(&key) {
        return Err(PlatformError::new(
            PlatformErrorCode::AlreadyInUse,
            operation,
            "logical Player resource is already open",
        ));
    }
    map.insert(key, value);
    Ok(())
}

fn lookup<K: Ord, V: Copy>(
    map: &BTreeMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<V, PlatformError> {
    map.get(key).copied().ok_or_else(|| {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            operation,
            "logical Player resource is not open",
        )
    })
}
