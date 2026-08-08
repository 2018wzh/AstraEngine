use std::{collections::BTreeMap, future::Future, pin::Pin};

use astra_platform::{
    DecodeKind, DecodeOutput, DecodeSessionHandle, PackageSourceHandle, PackageSourceRequest,
    PlatformDecodeRequest, PlatformError, PlatformErrorCode, PlatformHostClient,
    SaveTransactionHandle, SurfaceHandle,
};

use crate::{
    PlayerDecodeKind, PlayerHostCommand, PlayerHostCommandResult, PlayerHostCommandSink,
    PlayerHostResourceId, PlayerPackageSource,
};

pub struct PlatformCommandSink {
    client: PlatformHostClient,
    packages: BTreeMap<PlayerHostResourceId, PackageSourceHandle>,
    saves: BTreeMap<PlayerHostResourceId, SaveTransactionHandle>,
    decoders: BTreeMap<PlayerHostResourceId, DecodeSessionHandle>,
    surfaces: BTreeMap<PlayerHostResourceId, SurfaceHandle>,
}

impl PlatformCommandSink {
    pub fn new(client: PlatformHostClient) -> Self {
        Self {
            client,
            packages: BTreeMap::new(),
            saves: BTreeMap::new(),
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

    pub fn client(&self) -> &PlatformHostClient {
        &self.client
    }

    pub fn has_live_resources(&self) -> bool {
        !(self.packages.is_empty() && self.saves.is_empty() && self.decoders.is_empty())
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
                stream_action,
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
                            stream_action: decode_stream_action(*stream_action),
                            bytes: bytes.clone(),
                        },
                    )
                    .await?
                {
                    output @ (DecodeOutput::CpuBuffer { .. }
                    | DecodeOutput::AudioPcmI16 { .. }
                    | DecodeOutput::AudioPcmF32 { .. }
                    | DecodeOutput::VideoStreamStart { .. }
                    | DecodeOutput::VideoFrame { .. }
                    | DecodeOutput::VideoStreamEnd { .. }) => {
                        Ok(PlayerHostCommandResult::Decoded {
                            session: *session,
                            output,
                        })
                    }
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

fn decode_stream_action(
    action: crate::PlayerDecodeStreamAction,
) -> astra_platform::DecodeStreamAction {
    match action {
        crate::PlayerDecodeStreamAction::OneShot => astra_platform::DecodeStreamAction::OneShot,
        crate::PlayerDecodeStreamAction::Start => astra_platform::DecodeStreamAction::Start,
        crate::PlayerDecodeStreamAction::Next => astra_platform::DecodeStreamAction::Next,
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
