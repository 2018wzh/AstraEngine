//! Private, same-parent directory transactions. No private bytes precede the
//! staging directory's owner-only ACL; publication never replaces a target.
//! Windows requires owner ACLs and native file IDs. Linux/Android require procfs
//! for handle paths and kernel/filesystem RENAME_NOREPLACE support; macOS uses
//! F_GETPATH and RENAME_EXCL. Unsupported native operations fail without fallback.
use crate::{error::check_cancelled, PatchError, PatchErrorKind, PatchRequest};
use cap_std::{ambient_authority, fs::Dir};
use std::{
    ffi::OsString,
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(crate) struct Transaction {
    source: Dir,
    patch: Dir,
    parent: Dir,
    // Pin ancestors too: Windows capability operations resolve handle-derived
    // paths, and an ancestor rename must not redirect a later native operation.
    _pins: Vec<Dir>,
    staging: Option<Dir>,
    staging_name: OsString,
    output_name: OsString,
    #[cfg(test)]
    staging_path: PathBuf,
    published: bool,
}

impl Transaction {
    pub(crate) fn new(request: &PatchRequest) -> Result<Self, PatchError> {
        let source = Dir::open_ambient_dir(&request.source, ambient_authority())
            .map_err(|e| path_error("cannot open source directory", e))?;
        let patch = Dir::open_ambient_dir(&request.patch, ambient_authority())
            .map_err(|e| path_error("cannot open patch directory", e))?;
        let name = request.output.file_name().ok_or_else(|| {
            PatchError::new(
                PatchErrorKind::InvalidRequest,
                "output requires a directory name",
            )
        })?;
        let name_text = name
            .to_str()
            .ok_or_else(|| PatchError::new(PatchErrorKind::Path, "output name must be Unicode"))?;
        // Keep the relative-path policy in one place, including Windows ADS and
        // device names even when a transaction is prepared on another platform.
        let leaf = crate::paths::relative(name_text, false)?;
        if leaf.components().count() != 1 {
            return Err(PatchError::new(
                PatchErrorKind::Path,
                "output requires a single name",
            ));
        }
        let parent_path = request
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = Dir::open_ambient_dir(parent_path, ambient_authority())
            .map_err(|e| path_error("cannot open existing output parent", e))?;
        ensure_missing(&parent, Path::new(name))?;

        let source_file = source.try_clone()?.into_std_file();
        let patch_file = patch.try_clone()?.into_std_file();
        let mut pins = ancestors(&parent)?;
        for ancestor in &pins {
            let file = ancestor.try_clone()?.into_std_file();
            if same_file(&file, &source_file)? || same_file(&file, &patch_file)? {
                return Err(PatchError::new(
                    PatchErrorKind::InvalidRequest,
                    "output must not overlap either input directory",
                ));
            }
        }
        // An ancestor/equal output of an existing input necessarily already
        // exists. ensure_missing above rejects it using the opened parent,
        // including aliases, case-insensitive names and dangling links.
        pins.extend(ancestors(&source)?);
        pins.extend(ancestors(&patch)?);
        #[cfg(test)]
        let parent_final = final_dir_path(&parent)?;
        let (staging_name, staging) = create_staging(&parent)?;
        #[cfg(test)]
        let staging_path = parent_final.join(&staging_name);
        Ok(Self {
            source,
            patch,
            parent,
            _pins: pins,
            staging: Some(staging),
            staging_name,
            output_name: name.to_owned(),
            #[cfg(test)]
            staging_path,
            published: false,
        })
    }

    pub(crate) fn open_source(&self) -> Result<Dir, PatchError> {
        Ok(self.source.try_clone()?)
    }

    pub(crate) fn open_patch(&self) -> Result<Dir, PatchError> {
        Ok(self.patch.try_clone()?)
    }

    pub(crate) fn open_staging(&self) -> Result<Dir, PatchError> {
        Ok(self
            .staging
            .as_ref()
            .expect("unpublished transaction")
            .try_clone()?)
    }

    /// For diagnostics-free fixture assertions only; filesystem operations use
    /// the held directory capability, never this snapshot of its pathname.
    #[cfg(test)]
    pub(crate) fn staging_path(&self) -> &Path {
        &self.staging_path
    }

    /// All caller-owned staging directory/file handles must already be closed.
    pub(crate) fn publish(mut self, cancelled: &AtomicBool) -> Result<PathBuf, PatchError> {
        // Obtain everything fallible needed for the result before publication.
        // No cancellation check or other fallible operation follows the rename.
        let output = final_dir_path(&self.parent)?.join(&self.output_name);
        let identity = file_identity(
            &self
                .staging
                .as_ref()
                .expect("unpublished transaction")
                .try_clone()?
                .into_std_file(),
        )?;
        #[cfg(windows)]
        set_staging_hidden(
            self.staging.as_ref().expect("unpublished transaction"),
            false,
        )?;
        check_cancelled(cancelled)?;
        let staging = self.staging.take().expect("unpublished transaction");
        drop(staging); // Release our root handle before native publication.
        let mut cancelled_at_publish = false;
        let result = rename_noreplace_after(
            &self.parent,
            Path::new(&self.staging_name),
            &self.parent,
            Path::new(&self.output_name),
            || {
                // The native wrapper has finished path preparation. This is
                // the last check immediately before the atomic rename.
                cancelled_at_publish = cancelled.load(Ordering::Acquire);
                if cancelled_at_publish {
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "conversion cancelled",
                    ))
                } else {
                    Ok(())
                }
            },
        );
        if let Err(error) = result {
            self.staging = reopen_staging(&self.parent, &self.staging_name, identity).ok();
            if cancelled_at_publish {
                return Err(PatchError::new(
                    PatchErrorKind::Cancelled,
                    "conversion cancelled",
                ));
            }
            let kind = if target_exists_error(&error)
                || self.parent.symlink_metadata(&self.output_name).is_ok()
            {
                PatchErrorKind::OutputExists
            } else {
                PatchErrorKind::Publish
            };
            return Err(PatchError::with_source(
                kind,
                "cannot publish output directory",
                error,
            ));
        }
        self.published = true;
        Ok(output)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        if let Some(staging) = self.staging.take() {
            if let Err(error) = cleanup(staging) {
                // Preserve a useful diagnostic without a private path or error
                // string (some OS errors carry the path that caused them).
                eprintln!(
                    "ASTRA_EMU_PATCH_CLEANUP_FAILED (os_code={:?})",
                    error.raw_os_error()
                );
            }
        } else {
            eprintln!("ASTRA_EMU_PATCH_CLEANUP_FAILED (identity unavailable)");
        }
    }
}

fn path_error(message: &'static str, error: io::Error) -> PatchError {
    PatchError::with_source(PatchErrorKind::Path, message, error)
}

fn ensure_missing(parent: &Dir, name: &Path) -> Result<(), PatchError> {
    match parent.symlink_metadata(name) {
        Ok(_) => Err(PatchError::new(
            PatchErrorKind::OutputExists,
            "output already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(path_error("cannot inspect output target", error)),
    }
}

fn ancestors(directory: &Dir) -> io::Result<Vec<Dir>> {
    let mut result = Vec::new();
    let mut current = directory.try_clone()?;
    loop {
        let parent = current.open_parent_dir(ambient_authority())?;
        let is_root = same_file(
            &current.try_clone()?.into_std_file(),
            &parent.try_clone()?.into_std_file(),
        )?;
        result.push(current);
        if is_root {
            break;
        }
        current = parent;
    }
    Ok(result)
}

fn create_staging(parent: &Dir) -> io::Result<(OsString, Dir)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| io::Error::other("system clock precedes epoch"))?
        .as_nanos();
    for _ in 0..128 {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = OsString::from(format!(
            ".astra-patch-staging-{:x}-{time:x}-{serial:x}",
            std::process::id()
        ));
        match create_private_directory(parent, Path::new(&name)) {
            Ok(dir) => return Ok((name, dir)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate unique staging directory",
    ))
}

fn target_exists_error(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::AlreadyExists {
        return true;
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(80 | 183))
    }
    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(libc::EEXIST | libc::ENOTEMPTY))
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

#[cfg(unix)]
type FileIdentity = (u64, u64);
#[cfg(windows)]
type FileIdentity = (u64, [u8; 16]);

/// Compare identities of the opened objects, including case and mount aliases.
pub(crate) fn same_file(left: &File, right: &File) -> io::Result<bool> {
    Ok(file_identity(left)? == file_identity(right)?)
}

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO},
    };
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .map_err(win_error)?;
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

/// Derive a final, absolute path from the opened object, not the request string.
#[cfg(windows)]
pub(crate) fn final_path(file: &File) -> io::Result<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    final_path_handle(file.as_raw_handle())
}

#[cfg(windows)]
pub(crate) fn final_dir_path(directory: &Dir) -> io::Result<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    final_path_handle(directory.as_raw_handle())
}

#[cfg(not(windows))]
pub(crate) fn final_dir_path(directory: &Dir) -> io::Result<PathBuf> {
    final_path(&directory.try_clone()?.into_std_file())
}

#[cfg(windows)]
fn final_path_handle(handle: std::os::windows::io::RawHandle) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFinalPathNameByHandleW, GETFINALPATHNAMEBYHANDLE_FLAGS},
    };
    let mut buffer = vec![0u16; 512];
    loop {
        let count = unsafe {
            GetFinalPathNameByHandleW(
                HANDLE(handle),
                &mut buffer,
                GETFINALPATHNAMEBYHANDLE_FLAGS(0),
            )
        } as usize;
        if count == 0 {
            return Err(io::Error::last_os_error());
        }
        if count < buffer.len() {
            buffer.truncate(count);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(count + 1, 0);
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn final_path(file: &File) -> io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

#[cfg(target_os = "macos")]
pub(crate) fn final_path(file: &File) -> io::Result<PathBuf> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStringExt};
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| io::Error::other("invalid directory handle path"))?;
    buffer.truncate(length);
    Ok(PathBuf::from(OsString::from_vec(buffer)))
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
pub(crate) fn final_path(_file: &File) -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-derived paths unsupported on this platform",
    ))
}

#[cfg(windows)]
fn win_error(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32)
}

#[cfg(windows)]
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut result: Vec<u16> = path.as_os_str().encode_wide().collect();
    if result.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    result.push(0);
    Ok(result)
}

#[cfg(windows)]
fn create_private_directory(parent: &Dir, name: &Path) -> io::Result<Dir> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                },
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
            Storage::FileSystem::CreateDirectoryW,
        },
    };
    let path = wide(&final_dir_path(parent)?.join(name))?;
    // OW is Owner Rights. OI+CI make the owner-only ACE inherited by every
    // descendant. P prevents permissive ACLs from the output parent leaking in.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(win_error)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let created =
        unsafe { CreateDirectoryW(PCWSTR(path.as_ptr()), Some(&attributes)) }.map_err(win_error);
    unsafe {
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    created?;
    let directory = match open_windows_staging(parent, name) {
        Ok(directory) => directory,
        Err(error) => {
            // No handle identity: do not delete a possibly substituted name.
            eprintln!("ASTRA_EMU_PATCH_CLEANUP_FAILED (staging open failed)");
            return Err(error);
        }
    };
    if let Err(error) = set_staging_hidden(&directory, true) {
        if let Err(cleanup_error) = cleanup(directory) {
            eprintln!(
                "ASTRA_EMU_PATCH_CLEANUP_FAILED (os_code={:?})",
                cleanup_error.raw_os_error()
            );
        }
        return Err(error);
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_windows_staging(parent: &Dir, name: &Path) -> io::Result<Dir> {
    use std::os::windows::{fs::MetadataExt, io::FromRawHandle};
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
            OPEN_EXISTING,
        },
    };
    let path = wide(&final_dir_path(parent)?.join(name))?;
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            (DELETE | FILE_GENERIC_READ | FILE_WRITE_ATTRIBUTES).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(win_error)?;
    let file = unsafe { File::from_raw_handle(handle.0) };
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(io::Error::other("staging directory identity changed"));
    }
    Ok(Dir::from_std_file(file))
}

#[cfg(windows)]
fn set_staging_hidden(directory: &Dir, hidden: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileBasicInfo, SetFileInformationByHandle, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_HIDDEN, FILE_BASIC_INFO,
        },
    };
    let info = FILE_BASIC_INFO {
        FileAttributes: FILE_ATTRIBUTE_DIRECTORY.0
            | if hidden { FILE_ATTRIBUTE_HIDDEN.0 } else { 0 },
        ..Default::default()
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(directory.as_raw_handle()),
            FileBasicInfo,
            (&info as *const FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .map_err(win_error)
}

#[cfg(unix)]
fn create_private_directory(parent: &Dir, name: &Path) -> io::Result<Dir> {
    use cap_std::fs::{DirBuilder, DirBuilderExt};
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    parent.create_dir_with(name, &builder)?;
    parent.open_dir(name).inspect_err(|_| {
        eprintln!("ASTRA_EMU_PATCH_CLEANUP_FAILED (staging open failed)");
    })
}

/// Native, same-filesystem, atomic no-replace rename. No fallback may overwrite.
pub(crate) fn rename_noreplace(
    from_dir: &Dir,
    from: &Path,
    to_dir: &Dir,
    to: &Path,
) -> io::Result<()> {
    rename_noreplace_after(from_dir, from, to_dir, to, || Ok(()))
}

#[cfg(windows)]
fn rename_noreplace_after(
    from_dir: &Dir,
    from: &Path,
    to_dir: &Dir,
    to: &Path,
    before: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
    };
    let from = wide(&final_dir_path(from_dir)?.join(from))?;
    let to = wide(&final_dir_path(to_dir)?.join(to))?;
    before()?;
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(win_error)
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn rename_noreplace_after(
    from_dir: &Dir,
    from: &Path,
    to_dir: &Dir,
    to: &Path,
    before: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    use std::{
        ffi::CString,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename source contains NUL"))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename target contains NUL"))?;
    before()?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
fn rename_noreplace_after(
    _from_dir: &Dir,
    _from: &Path,
    _to_dir: &Dir,
    _to: &Path,
    _before: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename unsupported on this platform",
    ))
}

// Never call cap_std's Windows remove_open_dir_all: it closes the pin and then
// recursively deletes an ambient path. Delete descendants through capabilities,
// and mark the original staging object for deletion through its DELETE handle.
fn cleanup(directory: Dir) -> io::Result<()> {
    #[cfg(windows)]
    set_staging_hidden(&directory, true)?;
    clear_directory(&directory)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
            },
        };
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            SetFileInformationByHandle(
                HANDLE(directory.as_raw_handle()),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        }
        .map_err(win_error)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        directory.remove_open_dir()
    }
}

fn clear_directory(directory: &Dir) -> io::Result<()> {
    for entry in directory.entries()? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() && !kind.is_symlink() {
            #[cfg(windows)]
            {
                // Opening an entry through cap-std would re-open its ambient
                // parent without FILE_SHARE_DELETE, conflicting with our pinned
                // DELETE handle. Delete each child through its own native pin.
                let child = open_windows_staging(directory, Path::new(&entry.file_name()))?;
                cleanup(child)?;
            }
            #[cfg(not(windows))]
            {
                let child = entry.open_dir()?;
                clear_directory(&child)?;
                drop(child);
                entry.remove_dir()?;
            }
        } else if kind.is_symlink() {
            // Windows directory symlinks need RemoveDirectory, file symlinks
            // need DeleteFile. file_type().is_dir() does not dereference links.
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                let parent = final_path(&directory.try_clone()?.into_std_file())?;
                let metadata = std::fs::symlink_metadata(parent.join(entry.file_name()))?;
                if metadata.file_attributes() & 0x10 != 0 {
                    entry.remove_dir()?;
                } else {
                    entry.remove_file()?;
                }
            }
            #[cfg(not(windows))]
            entry.remove_file()?;
        } else {
            entry.remove_file()?;
        }
    }
    Ok(())
}

fn reopen_staging(parent: &Dir, name: &OsString, identity: FileIdentity) -> io::Result<Dir> {
    #[cfg(windows)]
    let directory = open_windows_staging(parent, Path::new(name))?;
    #[cfg(unix)]
    let directory = {
        let kind = parent.symlink_metadata(name)?.file_type();
        if kind.is_symlink() || !kind.is_dir() {
            return Err(io::Error::other("staging directory identity changed"));
        }
        parent.open_dir(name)?
    };
    if file_identity(&directory.try_clone()?.into_std_file())? != identity {
        return Err(io::Error::other("staging directory identity changed"));
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        directory: tempfile::TempDir,
        request: PatchRequest,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("原始资源");
            let patch = directory.path().join("补丁");
            std::fs::create_dir(&source).unwrap();
            std::fs::create_dir(&patch).unwrap();
            std::fs::write(source.join("original.bin"), b"source unchanged").unwrap();
            std::fs::write(patch.join("patch.bin"), b"patch unchanged").unwrap();
            let request = PatchRequest {
                script: directory.path().join("fixture.luau"),
                source,
                patch,
                output: directory.path().join("独立输出"),
            };
            Self { directory, request }
        }

        fn assert_inputs(&self) {
            assert_eq!(
                std::fs::read(self.request.source.join("original.bin")).unwrap(),
                b"source unchanged"
            );
            assert_eq!(
                std::fs::read(self.request.patch.join("patch.bin")).unwrap(),
                b"patch unchanged"
            );
        }

        fn assert_no_staging(&self) {
            assert!(!std::fs::read_dir(self.directory.path())
                .unwrap()
                .any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".astra-patch-staging-")
                }));
        }
    }

    #[test]
    fn accepts_identical_inputs_and_publishes_independent_unicode_output() {
        let mut fixture = Fixture::new();
        fixture.request.patch = fixture.request.source.clone();
        let transaction = Transaction::new(&fixture.request).unwrap();
        let staging = transaction.open_staging().unwrap();
        staging.write("输出.bin", b"independent bytes").unwrap();
        drop(staging);
        let output = transaction.publish(&AtomicBool::new(false)).unwrap();
        assert_eq!(
            std::fs::canonicalize(&output).unwrap(),
            std::fs::canonicalize(&fixture.request.output).unwrap()
        );
        assert_eq!(
            std::fs::read(output.join("输出.bin")).unwrap(),
            b"independent bytes"
        );
        assert_eq!(
            std::fs::read(fixture.request.source.join("original.bin")).unwrap(),
            b"source unchanged"
        );
        fixture.assert_no_staging();
    }

    #[test]
    fn rejects_each_input_overlap_before_creating_staging() {
        let mut fixture = Fixture::new();
        for output in [
            fixture.request.source.join("new-output"),
            fixture.request.patch.join("new-output"),
            fixture.request.source.clone(),
            fixture.directory.path().to_owned(),
        ] {
            fixture.request.output = output;
            assert!(Transaction::new(&fixture.request).is_err());
            fixture.assert_no_staging();
            fixture.assert_inputs();
        }
    }

    #[test]
    fn missing_input_or_parent_creates_nothing() {
        let mut fixture = Fixture::new();
        fixture.request.output = fixture.directory.path().join("missing").join("output");
        assert!(Transaction::new(&fixture.request).is_err());
        assert!(!fixture.directory.path().join("missing").exists());
        fixture.request.output = fixture.directory.path().join("output");
        fixture.request.source = fixture.directory.path().join("absent-source");
        assert!(Transaction::new(&fixture.request).is_err());
        fixture.assert_no_staging();
    }

    #[test]
    fn existing_output_is_preserved_byte_for_byte() {
        let fixture = Fixture::new();
        std::fs::create_dir(&fixture.request.output).unwrap();
        std::fs::write(fixture.request.output.join("user.bin"), b"user data").unwrap();
        let error = Transaction::new(&fixture.request).err().unwrap();
        assert_eq!(error.kind(), PatchErrorKind::OutputExists);
        assert_eq!(
            std::fs::read(fixture.request.output.join("user.bin")).unwrap(),
            b"user data"
        );
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[test]
    fn staging_is_private_before_writing_and_descendants_inherit_on_windows() {
        let fixture = Fixture::new();
        let transaction = Transaction::new(&fixture.request).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(transaction.staging_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            let staging = transaction.open_staging().unwrap();
            assert_owner_acl(&staging.try_clone().unwrap().into_std_file(), true);
            assert_ne!(
                std::fs::metadata(transaction.staging_path())
                    .unwrap()
                    .file_attributes()
                    & 2,
                0
            );
            staging.create_dir("child").unwrap();
            let child = staging.open_dir("child").unwrap();
            assert_owner_acl(&child.into_std_file(), false);
            let file = staging.create("private.bin").unwrap();
            assert_owner_acl(&file.into_std(), false);
        }
        drop(transaction);
        fixture.assert_no_staging();
    }

    #[test]
    fn cancellation_removes_written_staging_without_touching_inputs() {
        let fixture = Fixture::new();
        let transaction = Transaction::new(&fixture.request).unwrap();
        let staging_path = transaction.staging_path().to_owned();
        let staging = transaction.open_staging().unwrap();
        staging.create_dir("nested").unwrap();
        staging
            .write("nested/partial.bin", b"private partial output")
            .unwrap();
        drop(staging);
        let error = transaction.publish(&AtomicBool::new(true)).unwrap_err();
        assert_eq!(error.kind(), PatchErrorKind::Cancelled);
        assert!(!staging_path.exists());
        assert!(!fixture.request.output.exists());
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[test]
    fn raced_output_target_is_not_overwritten_and_failed_staging_is_removed() {
        let fixture = Fixture::new();
        let transaction = Transaction::new(&fixture.request).unwrap();
        let staging = transaction.open_staging().unwrap();
        staging.write("new.bin", b"converted output").unwrap();
        drop(staging);
        std::fs::create_dir(&fixture.request.output).unwrap();
        std::fs::write(fixture.request.output.join("user.bin"), b"race winner").unwrap();
        let error = transaction.publish(&AtomicBool::new(false)).unwrap_err();
        assert_eq!(error.kind(), PatchErrorKind::OutputExists);
        assert_eq!(
            std::fs::read(fixture.request.output.join("user.bin")).unwrap(),
            b"race winner"
        );
        assert!(!fixture.request.output.join("new.bin").exists());
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[test]
    fn publication_remains_successful_after_late_cancellation() {
        let fixture = Fixture::new();
        let cancelled = AtomicBool::new(false);
        let transaction = Transaction::new(&fixture.request).unwrap();
        transaction
            .open_staging()
            .unwrap()
            .write("complete.bin", b"complete")
            .unwrap();
        let output = transaction.publish(&cancelled).unwrap();
        cancelled.store(true, Ordering::Release);
        assert_eq!(
            std::fs::read(output.join("complete.bin")).unwrap(),
            b"complete"
        );
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[test]
    fn native_file_rename_does_not_replace_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(directory.path(), ambient_authority()).unwrap();
        root.create_dir("from").unwrap();
        root.create_dir("to").unwrap();
        let from = root.open_dir("from").unwrap();
        let to = root.open_dir("to").unwrap();
        from.write("item", b"new bytes").unwrap();
        to.write("item", b"existing bytes").unwrap();
        assert!(rename_noreplace(&from, Path::new("item"), &to, Path::new("item")).is_err());
        assert_eq!(from.read("item").unwrap(), b"new bytes");
        assert_eq!(to.read("item").unwrap(), b"existing bytes");
    }

    #[cfg(unix)]
    #[test]
    fn aliased_input_ancestor_is_rejected_by_identity() {
        use std::os::unix::fs::symlink;
        let mut fixture = Fixture::new();
        let alias = fixture.directory.path().join("alias");
        symlink(&fixture.request.source, &alias).unwrap();
        fixture.request.output = alias.join("output");
        assert_eq!(
            Transaction::new(&fixture.request).err().unwrap().kind(),
            PatchErrorKind::InvalidRequest
        );
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_uses_held_directory_not_substituted_staging_symlink() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let transaction = Transaction::new(&fixture.request).unwrap();
        let original_name = transaction.staging_path().to_owned();
        let moved = fixture.directory.path().join("moved-staging");
        transaction
            .open_staging()
            .unwrap()
            .write("partial.bin", b"partial")
            .unwrap();
        std::fs::rename(&original_name, &moved).unwrap();
        symlink(&fixture.request.source, &original_name).unwrap();
        drop(transaction);
        assert!(!moved.exists());
        assert!(std::fs::symlink_metadata(original_name)
            .unwrap()
            .file_type()
            .is_symlink());
        fixture.assert_inputs();
    }

    #[cfg(windows)]
    #[test]
    fn windows_staging_and_parent_are_pinned_against_rename() {
        let fixture = Fixture::new();
        let transaction = Transaction::new(&fixture.request).unwrap();
        assert!(std::fs::rename(
            transaction.staging_path(),
            fixture.directory.path().join("substitute")
        )
        .is_err());
        assert!(std::fs::rename(
            fixture.directory.path(),
            fixture.directory.path().with_extension("renamed")
        )
        .is_err());
        drop(transaction);
        fixture.assert_inputs();
        fixture.assert_no_staging();
    }

    #[cfg(windows)]
    fn assert_owner_acl(file: &File, protected: bool) {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::{LocalFree, HANDLE, HLOCAL},
            Security::{
                Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
                CreateWellKnownSid, EqualSid, GetAce, GetSecurityDescriptorControl,
                WinCreatorOwnerRightsSid, ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
            },
        };
        unsafe {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            let mut acl: *mut ACL = std::ptr::null_mut();
            GetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut acl),
                None,
                Some(&mut descriptor),
            )
            .ok()
            .unwrap();
            let mut control = 0;
            let mut revision = 0;
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision).unwrap();
            assert_eq!(control & SE_DACL_PROTECTED.0 != 0, protected);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            let mut ace = std::ptr::null_mut();
            GetAce(acl, 0, &mut ace).unwrap();
            let ace = &*(ace as *const ACCESS_ALLOWED_ACE);
            assert_eq!(ace.Header.AceType, 0);
            assert_eq!(ace.Mask, 0x001f01ff);
            if protected {
                assert_eq!(ace.Header.AceFlags & 3, 3);
            } else {
                assert_ne!(ace.Header.AceFlags & 0x10, 0);
            }
            let mut sid_buffer = [0u32; 17];
            let mut sid_length = std::mem::size_of_val(&sid_buffer) as u32;
            let sid = PSID(sid_buffer.as_mut_ptr().cast());
            CreateWellKnownSid(WinCreatorOwnerRightsSid, None, Some(sid), &mut sid_length).unwrap();
            EqualSid(
                PSID(std::ptr::addr_of!(ace.SidStart).cast_mut().cast()),
                sid,
            )
            .unwrap();
            LocalFree(Some(HLOCAL(descriptor.0)));
        }
    }
}
