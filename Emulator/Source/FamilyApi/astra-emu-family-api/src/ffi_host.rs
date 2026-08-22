use std::sync::Arc;

use abi_stable::std_types::RResult;
use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};

use crate::*;

#[derive(Clone)]
pub struct FfiLegacyFamilyHostAdapter {
    services: FfiLegacyHostServices,
}

impl FfiLegacyFamilyHostAdapter {
    pub fn new(services: FfiLegacyHostServices) -> Self {
        Self { services }
    }

    pub fn into_host_services(self) -> LegacyFamilyHostServicesV9 {
        let shared = Arc::new(self);
        LegacyFamilyHostServicesV9 {
            vfs: shared.clone(),
            surfaces: shared.clone(),
            hooks: shared.clone(),
            writable_files: shared,
        }
    }
}

impl LegacyVfsReader for FfiLegacyFamilyHostAdapter {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<ByteSourceStat, LegacyProviderError> {
        native_result((self.services.stat_vfs)(
            self.services.host_token.clone(),
            FfiVfsStatCall {
                mount_set_id: mount_set_id.into(),
                uri: uri.into(),
            },
        ))
    }

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, LegacyProviderError> {
        let result: RangeReadResult = native_result((self.services.read_vfs_range)(
            self.services.host_token.clone(),
            FfiVfsRangeCall {
                mount_set_id: mount_set_id.into(),
                uri: uri.into(),
                expected_revision: expected_revision.0,
                range: range.into(),
                max_bytes,
            },
        ))?;
        if result.range != range
            || result.revision != expected_revision
            || result.bytes.len() as u64 != range.len
            || result.bytes.len() as u64 > max_bytes
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_VFS_BOUNDS",
                "host VFS result does not match the requested range",
            ));
        }
        Ok(result)
    }

    fn enumerate_by_extension(
        &self,
        mount_set_id: &str,
        root: &str,
        extension_without_dot: &str,
        max_entries: u32,
    ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
        let entries = match (self.services.enumerate_vfs)(
            self.services.host_token.clone(),
            FfiVfsEnumerateCall {
                mount_set_id: mount_set_id.into(),
                root: root.into(),
                extension_without_dot: extension_without_dot.into(),
                max_entries,
            },
        ) {
            RResult::ROk(entries) => entries.into_iter().map(Into::into).collect::<Vec<_>>(),
            RResult::RErr(error) => return Err(error.into()),
        };
        if entries.len() > max_entries as usize {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_VFS_ENUM_BOUNDS",
                "host VFS enumeration exceeded the requested bound",
            ));
        }
        Ok(entries)
    }
}

impl LegacySurfaceHostV9 for FfiLegacyFamilyHostAdapter {
    fn acquire(
        &self,
        session_id: &str,
        fixed_step: u64,
        surface_id: &str,
        width: u32,
        height: u32,
        format: LegacySurfaceFormatV9,
    ) -> Result<LegacySurfaceLeaseV9, LegacyProviderError> {
        let lease: LegacySurfaceLeaseV9 =
            native_result((self.services.acquire_surface)(FfiAcquireSurfaceCallV9 {
                host_token: self.services.host_token.clone(),
                session_id: session_id.into(),
                fixed_step,
                surface_id: surface_id.into(),
                width,
                height,
                format: match format {
                    LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => {
                        FfiSurfaceFormatV9::Rgba8SrgbPremultiplied
                    }
                    LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => {
                        FfiSurfaceFormatV9::Bgra8SrgbPremultiplied
                    }
                },
            }))?;
        lease.validate()?;
        Ok(lease)
    }

    fn commit(
        &self,
        session_id: &str,
        fixed_step: u64,
        commit: LegacySurfaceCommitV9,
    ) -> Result<(), LegacyProviderError> {
        commit.validate()?;
        native_result((self.services.commit_surface)(FfiCommitSurfaceCallV9 {
            host_token: self.services.host_token.clone(),
            session_id: session_id.into(),
            fixed_step,
            lease: commit.lease.into(),
            damage: commit.damage.into(),
        }))
    }
}

impl LegacyHookHostV1 for FfiLegacyFamilyHostAdapter {
    fn invoke(
        &self,
        invocation: LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        let result = match (self.services.invoke_hook)(FfiHookInvocationV1 {
            host_token: self.services.host_token.clone(),
            session_id: invocation.session_id.into(),
            invocation_id: invocation.invocation_id.into(),
            family_id: invocation.family_id.into(),
            family_game_id: invocation.family_game_id.into(),
            hook_id: invocation.hook_id.into(),
            timeout_ms: invocation.timeout_ms,
            payload: invocation.payload.into_ffi(),
        }) {
            RResult::ROk(result) => result,
            RResult::RErr(error) => return Err(error.into()),
        };
        Ok(LegacyHookResultV1 {
            status: match result.status {
                FfiHookStatusV1::Completed => LegacyHookStatusV1::Completed,
                FfiHookStatusV1::Unbound => LegacyHookStatusV1::Unbound,
                FfiHookStatusV1::TimedOut => LegacyHookStatusV1::TimedOut,
                FfiHookStatusV1::Failed => LegacyHookStatusV1::Failed,
            },
            payload: result.payload.into_owned(),
            diagnostics: result.diagnostics.into_iter().map(Into::into).collect(),
        })
    }
}

impl LegacyWritableFileHostV1 for FfiLegacyFamilyHostAdapter {
    fn execute(
        &self,
        session_id: &str,
        request: LegacyWritableFileRequestV1,
    ) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
        request.validate()?;
        let request = match request {
            LegacyWritableFileRequestV1::Stat { path } => {
                FfiWritableFileRequestV1::Stat { path: path.into() }
            }
            LegacyWritableFileRequestV1::List { path } => {
                FfiWritableFileRequestV1::List { path: path.into() }
            }
            LegacyWritableFileRequestV1::CreateDir { path } => {
                FfiWritableFileRequestV1::CreateDir { path: path.into() }
            }
            LegacyWritableFileRequestV1::ReadRange {
                path,
                offset,
                length,
            } => FfiWritableFileRequestV1::ReadRange {
                path: path.into(),
                offset,
                length,
            },
            LegacyWritableFileRequestV1::WriteRange {
                path,
                offset,
                bytes,
            } => FfiWritableFileRequestV1::WriteRange {
                path: path.into(),
                offset,
                bytes: astra_byte_source::OwnedByteBuffer::from(bytes).into_ffi(),
            },
            LegacyWritableFileRequestV1::SetLength { path, length } => {
                FfiWritableFileRequestV1::SetLength {
                    path: path.into(),
                    length,
                }
            }
            LegacyWritableFileRequestV1::Remove { path } => {
                FfiWritableFileRequestV1::Remove { path: path.into() }
            }
            LegacyWritableFileRequestV1::AtomicReplace {
                temporary_path,
                destination_path,
            } => FfiWritableFileRequestV1::AtomicReplace {
                temporary_path: temporary_path.into(),
                destination_path: destination_path.into(),
            },
        };
        let result = match (self.services.writable_file)(FfiWritableFileCallV1 {
            host_token: self.services.host_token.clone(),
            session_id: session_id.into(),
            request,
        }) {
            RResult::ROk(result) => result,
            RResult::RErr(error) => return Err(error.into()),
        };
        Ok(LegacyWritableFileResultV1 {
            exists: result.exists,
            is_file: result.is_file,
            length: result.length,
            entries: result
                .entries
                .into_iter()
                .map(|entry| LegacyWritableFileEntryV1 {
                    name: entry.name.to_string(),
                    is_file: entry.is_file,
                    length: entry.length,
                })
                .collect(),
            bytes: result.bytes.into_owned(),
            written: result.written,
        })
    }
}
