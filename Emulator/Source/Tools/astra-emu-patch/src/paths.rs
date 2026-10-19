use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, DirBuilder, Metadata, OpenOptions, OpenOptionsExt};
use serde::Serialize;

use crate::{PatchError, PatchErrorKind};

/// Script paths have one spelling on every host; never normalize an invalid spelling.
pub(crate) fn relative(value: &str, allow_empty: bool) -> Result<PathBuf, PatchError> {
    if value.is_empty() {
        return if allow_empty {
            Ok(PathBuf::new())
        } else {
            Err(path_error("empty relative path"))
        };
    }
    if value.contains(['\\', '\0', ':']) {
        return Err(path_error("relative path contains a forbidden character"));
    }
    for component in value.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
        {
            return Err(path_error("relative path is not canonical"));
        }
        // Win32 recognizes device names even with an extension, and recognizes
        // superscript 1/2/3 in COM/LPT names. Do not let Unix recipes create them.
        let stem = component
            .split('.')
            .next()
            .unwrap_or(component)
            .trim_end_matches(' ');
        let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
            .iter()
            .any(|name| stem.eq_ignore_ascii_case(name));
        let port = stem.get(..3).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
        }) && matches!(
            stem.get(3..),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        );
        if device || port {
            return Err(path_error("relative path uses a reserved device name"));
        }
        if component
            .chars()
            .any(|ch| ch <= '\u{1f}' || matches!(ch, '<' | '>' | '"' | '|' | '?' | '*'))
        {
            return Err(path_error("relative path contains a forbidden character"));
        }
    }
    Ok(PathBuf::from(value))
}

#[derive(Serialize)]
pub(crate) struct DirectoryEntry {
    pub name: String,
    pub kind: String,
    pub size: u64,
}

/// The writable constructor accepts only an exclusively owned private staging
/// directory. This is not isolation against another process running as its owner.
/// No operation creates links, removes directories, or renames directories.
pub(crate) struct AuthorizedRoot {
    dir: Dir,
    writable: bool,
}

impl AuthorizedRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, PatchError> {
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|error| io_error("opening authorized input directory failed", error))?;
        Ok(Self::from_input_dir(dir))
    }

    pub(crate) fn from_input_dir(dir: Dir) -> Self {
        Self {
            dir,
            writable: false,
        }
    }

    pub(crate) fn from_dir(dir: Dir) -> Self {
        Self {
            dir,
            writable: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn try_clone(&self) -> Result<Self, PatchError> {
        Ok(Self {
            dir: self
                .dir
                .try_clone()
                .map_err(|error| io_error("cloning authorized directory failed", error))?,
            writable: self.writable,
        })
    }

    pub(crate) fn open_read(&self, value: &str) -> Result<File, PatchError> {
        let path = relative(value, false)?;
        let file = self
            .dir
            .open_with(path, &read_options())
            .map_err(|error| io_error("opening input file failed", error))?
            .into_std();
        self.verify_bounded(&file)?;
        regular_file(&file, false)?;
        Ok(file)
    }

    pub(crate) fn exists(&self, value: &str) -> Result<bool, PatchError> {
        let path = relative(value, false)?;
        let mut options = read_options();
        options.maybe_dir(true);
        let file = match self.dir.open_with(path, &options) {
            Ok(file) => file.into_std(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error("checking input existence failed", error)),
        };
        self.verify_bounded(&file)?;
        let metadata = file
            .metadata()
            .map_err(|error| io_error("reading opened file metadata failed", error))?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(path_error("input is not an ordinary file or directory"));
        }
        Ok(true)
    }

    pub(crate) fn list(&self, value: &str) -> Result<Vec<DirectoryEntry>, PatchError> {
        let path = relative(value, true)?;
        // Walk one component at a time: no intermediate directory link is
        // followed, including links whose target happens to be inside the root.
        let directory = self.directory(&path, false)?;
        let mut result = Vec::new();
        for entry in directory
            .entries()
            .map_err(|error| io_error("enumerating directory failed", error))?
        {
            let entry = entry.map_err(|error| io_error("reading directory entry failed", error))?;
            let metadata = entry
                .metadata()
                .map_err(|error| io_error("reading directory entry metadata failed", error))?;
            let (kind, size) = if is_link(&metadata) {
                ("link", 0)
            } else if metadata.is_file() {
                ("file", metadata.len())
            } else if metadata.is_dir() {
                ("directory", 0)
            } else {
                return Err(path_error("directory contains a nonregular special entry"));
            };
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| path_error("directory entry name is not Unicode"))?;
            result.push(DirectoryEntry {
                name,
                kind: kind.into(),
                size,
            });
        }
        result.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Ok(result)
    }

    pub(crate) fn create(&self, value: &str) -> Result<File, PatchError> {
        self.require_writable()?;
        let path = relative(value, false)?;
        let parent = self.directory(path.parent().unwrap_or(Path::new("")), false)?;
        let mut options = output_options();
        options.create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = parent
            .open_with(path.file_name().expect("validated nonempty path"), &options)
            .map_err(|error| output_error("creating output file failed", error))?
            .into_std();
        self.verify_bounded(&file)?;
        regular_file(&file, true)?;
        Ok(file)
    }

    pub(crate) fn edit(&self, value: &str) -> Result<File, PatchError> {
        self.require_writable()?;
        let path = relative(value, false)?;
        let parent = self.directory(path.parent().unwrap_or(Path::new("")), false)?;
        let file = parent
            .open_with(
                path.file_name().expect("validated nonempty path"),
                &output_options(),
            )
            .map_err(|error| io_error("opening output file failed", error))?
            .into_std();
        self.verify_bounded(&file)?;
        regular_file(&file, true)?;
        Ok(file)
    }

    pub(crate) fn mkdir(&self, value: &str) -> Result<(), PatchError> {
        self.require_writable()?;
        let path = relative(value, false)?;
        self.directory(&path, true)?;
        Ok(())
    }

    pub(crate) fn remove(&self, value: &str) -> Result<(), PatchError> {
        self.require_writable()?;
        let path = relative(value, false)?;
        let parent = self.directory(path.parent().unwrap_or(Path::new("")), false)?;
        let leaf = Path::new(path.file_name().expect("validated nonempty path"));
        self.inspect_output(&parent, leaf)?;
        parent
            .remove_file(leaf)
            .map_err(|error| io_error("removing output file failed", error))
    }

    pub(crate) fn rename(&self, from: &str, to: &str) -> Result<(), PatchError> {
        self.require_writable()?;
        let from = relative(from, false)?;
        let to = relative(to, false)?;
        let from_parent = self.directory(from.parent().unwrap_or(Path::new("")), false)?;
        let to_parent = self.directory(to.parent().unwrap_or(Path::new("")), false)?;
        let from_leaf = Path::new(from.file_name().expect("validated nonempty path"));
        let to_leaf = Path::new(to.file_name().expect("validated nonempty path"));
        self.inspect_output(&from_parent, from_leaf)?;
        // Atomic native no-replace, not a racy exists-then-rename sequence.
        crate::transaction::rename_noreplace(&from_parent, from_leaf, &to_parent, to_leaf)
            .map_err(|error| output_error("renaming output file failed", error))
    }

    fn inspect_output(&self, parent: &Dir, leaf: &Path) -> Result<(), PatchError> {
        let mut options = read_options();
        options.follow(FollowSymlinks::No);
        reparse_handle(&mut options);
        let file = parent
            .open_with(leaf, &options)
            .map_err(|error| io_error("opening output file for inspection failed", error))?
            .into_std();
        self.verify_bounded(&file)?;
        regular_file(&file, true)
    }

    fn require_writable(&self) -> Result<(), PatchError> {
        if self.writable {
            Ok(())
        } else {
            Err(path_error("input roots are read-only"))
        }
    }

    fn directory(&self, path: &Path, create: bool) -> Result<Dir, PatchError> {
        let mut directory = self
            .dir
            .try_clone()
            .map_err(|error| io_error("cloning authorized directory failed", error))?;
        for component in path.components() {
            let component = component.as_os_str();
            if create {
                #[cfg(unix)]
                let builder = {
                    use cap_std::fs::DirBuilderExt;
                    let mut builder = DirBuilder::new();
                    builder.mode(0o700);
                    builder
                };
                #[cfg(not(unix))]
                let builder = DirBuilder::new();
                match directory.create_dir_with(component, &builder) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(io_error("creating output directory failed", error)),
                }
            }
            let next = directory
                .open_dir_nofollow(component)
                .map_err(|error| io_error("opening ordinary directory component failed", error))?;
            let metadata = next
                .dir_metadata()
                .map_err(|error| io_error("reading opened directory metadata failed", error))?;
            if is_link(&metadata) || !metadata.is_dir() {
                return Err(path_error(
                    "directory traversal through a link or reparse point is forbidden",
                ));
            }
            directory = next;
        }
        Ok(directory)
    }

    fn verify_bounded(&self, file: &File) -> Result<(), PatchError> {
        // cap-std performs handle-relative bounded resolution on every host.
        // Windows additionally verifies the final opened handle because reparse
        // tags extend beyond the symbolic links recognized by std::FileType.
        #[cfg(windows)]
        {
            let root = crate::transaction::final_dir_path(&self.dir)
                .map_err(|error| io_error("resolving authorized directory handle failed", error))?;
            let opened = crate::transaction::final_path(file)
                .map_err(|error| io_error("resolving opened file handle failed", error))?;
            if opened.strip_prefix(&root).is_err() {
                return Err(path_error("opened handle is outside its authorized root"));
            }
        }
        #[cfg(not(windows))]
        let _ = file;
        Ok(())
    }
}

fn read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    // Reject FIFO/device inputs after opening without blocking on a FIFO.
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    options
}

fn output_options() -> OpenOptions {
    let mut options = read_options();
    options.write(true).follow(FollowSymlinks::No);
    reparse_handle(&mut options);
    options
}

fn reparse_handle(options: &mut OpenOptions) {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        // Open the reparse point itself, then reject every reparse tag from
        // opened-handle attributes. Never resolve an output reparse point.
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    #[cfg(not(windows))]
    let _ = options;
}

fn regular_file(file: &File, output: bool) -> Result<(), PatchError> {
    let metadata = Metadata::from_file(file)
        .map_err(|error| io_error("reading opened file metadata failed", error))?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(path_error("opened handle is not an ordinary file"));
    }
    if output && cap_fs_ext::MetadataExt::nlink(&metadata) != 1 {
        return Err(path_error("output hard links are forbidden"));
    }
    Ok(())
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_symlink()
    }
}

fn path_error(message: &'static str) -> PatchError {
    PatchError::new(PatchErrorKind::Path, message)
}

fn io_error(message: &'static str, error: io::Error) -> PatchError {
    PatchError::with_source(PatchErrorKind::Io, message, error)
}

fn output_error(message: &'static str, error: io::Error) -> PatchError {
    let kind = if error.kind() == io::ErrorKind::AlreadyExists {
        PatchErrorKind::OutputExists
    } else {
        PatchErrorKind::Io
    };
    PatchError::with_source(kind, message, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
    };

    fn staging(path: &Path) -> AuthorizedRoot {
        AuthorizedRoot::from_dir(Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap())
    }

    #[test]
    fn canonical_paths_reject_cross_platform_aliases() {
        for value in [
            "",
            "/a",
            "a/",
            "a//b",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "a\\b",
            "a\0b",
            "C:a",
            "C:/a",
            "//host/a",
            "\\\\?\\C:\\a",
            "a:stream",
            "a.",
            "a ",
            "NUL",
            "con.txt",
            "a/COM1.bin",
            "lpt²",
            "CONIN$",
            "CONOUT$.txt",
            "a?b",
            "a\nb",
        ] {
            assert!(relative(value, false).is_err(), "accepted invalid spelling");
        }
        assert!(relative("", true).unwrap().as_os_str().is_empty());
        for value in ["目录/文字.bin", "nulled", "com10", "a..b", "é/零字节"] {
            assert_eq!(relative(value, false).unwrap(), Path::new(value));
        }
    }

    #[test]
    fn ordinary_unicode_and_zero_files_preserve_input_authority() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("零字节"), []).unwrap();
        fs::write(temp.path().join("文字.bin"), b"public fixture").unwrap();
        let root = AuthorizedRoot::open(temp.path()).unwrap();
        assert_eq!(
            root.open_read("零字节").unwrap().metadata().unwrap().len(),
            0
        );
        let mut text = String::new();
        root.open_read("文字.bin")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "public fixture");
        assert!(root.exists("文字.bin").unwrap());
        assert!(!root.exists("missing").unwrap());
        assert!(root.exists("文字.bin/child").is_err());
        assert!(root.exists("../missing").is_err());
        assert!(root.open_read("").is_err());
        assert!(root.create("forbidden").is_err());
        assert!(root.try_clone().unwrap().mkdir("forbidden").is_err());
        assert!(root.edit("文字.bin").is_err());
        assert!(root.remove("文字.bin").is_err());
        assert!(root.rename("文字.bin", "renamed").is_err());
        assert_eq!(root.list("").unwrap().len(), 2);
    }

    #[test]
    fn output_operations_never_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let root = staging(temp.path());
        root.mkdir("目录/sub").unwrap();
        root.mkdir("目录/sub").unwrap();
        root.create("目录/sub/a")
            .unwrap()
            .write_all(b"one")
            .unwrap();
        root.create("目录/sub/b")
            .unwrap()
            .write_all(b"two")
            .unwrap();
        assert_eq!(
            root.create("目录/sub/a").unwrap_err().kind(),
            PatchErrorKind::OutputExists
        );
        assert_eq!(
            root.rename("目录/sub/a", "目录/sub/b").unwrap_err().kind(),
            PatchErrorKind::OutputExists
        );
        assert_eq!(fs::read(temp.path().join("目录/sub/b")).unwrap(), b"two");
        root.rename("目录/sub/a", "目录/c").unwrap();
        root.edit("目录/c").unwrap().set_len(0).unwrap();
        assert_eq!(
            root.open_read("目录/c").unwrap().metadata().unwrap().len(),
            0
        );
        assert!(root.remove("目录/sub").is_err());
        root.remove("目录/c").unwrap();
        assert!(!root.exists("目录/c").unwrap());
        assert!(root.create("missing/file").is_err());
        assert!(root.list("目录/").is_err());
        fs::hard_link(temp.path().join("目录/sub/b"), temp.path().join("hard")).unwrap();
        assert!(root.edit("hard").is_err());
        assert!(root.remove("hard").is_err());
        assert!(root.rename("hard", "hard-renamed").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(temp.path().join("目录/sub/b"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn concurrent_renames_have_exactly_one_winner() {
        use std::sync::{Arc, Barrier};
        let temp = tempfile::tempdir().unwrap();
        let root = staging(temp.path());
        root.create("first").unwrap().write_all(b"first").unwrap();
        root.create("second").unwrap().write_all(b"second").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let threads: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|source| {
                let root = root.try_clone().unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    root.rename(source, "winner")
                })
            })
            .collect();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .filter(|error| error.kind() == PatchErrorKind::OutputExists)
                .count(),
            1
        );
        let winner = fs::read(temp.path().join("winner")).unwrap();
        assert!(winner == b"first" || winner == b"second");
        assert_eq!(root.list("").unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_enumerated_but_only_bounded_inputs_can_be_followed() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("real")).unwrap();
        fs::write(temp.path().join("real/file"), b"inside").unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        symlink("real/file", temp.path().join("a-link")).unwrap();
        symlink("real", temp.path().join("b-dir-link")).unwrap();
        symlink(outside.path(), temp.path().join("escape")).unwrap();
        let root = AuthorizedRoot::open(temp.path()).unwrap();
        assert_eq!(
            root.open_read("a-link").unwrap().metadata().unwrap().len(),
            6
        );
        assert!(root.open_read("escape/file").is_err());
        assert!(root.exists("escape/file").is_err());
        assert!(root.list("b-dir-link").is_err());
        assert!(root.list("b-dir-link/nested").is_err());
        let entries = root.list("").unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["a-link", "b-dir-link", "escape", "real"]
        );
        assert_eq!(entries[0].kind, "link");
        assert_eq!(entries[0].size, 0);
        let output = staging(temp.path());
        assert!(output.edit("a-link").is_err());
        assert!(output.create("b-dir-link/new").is_err());
        assert!(output.mkdir("b-dir-link/new").is_err());
        assert!(output.remove("a-link").is_err());
        assert!(output.rename("a-link", "new").is_err());
        fs::hard_link(temp.path().join("real/file"), temp.path().join("hard")).unwrap();
        assert!(output.edit("hard").is_err());
        assert!(output.remove("hard").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn permission_failures_are_not_missing() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        // Root bypasses Unix discretionary permissions; do not pretend that
        // chmod creates an inaccessible fixture for a privileged test runner.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        fs::create_dir(temp.path().join("locked")).unwrap();
        let locked = temp.path().join("locked");
        let original = fs::metadata(&locked).unwrap().mode();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0)).unwrap();
        let result = AuthorizedRoot::open(temp.path())
            .unwrap()
            .exists("locked/file");
        fs::set_permissions(&locked, fs::Permissions::from_mode(original)).unwrap();
        assert_eq!(result.unwrap_err().kind(), PatchErrorKind::Io);
    }

    #[cfg(unix)]
    #[test]
    fn special_entries_fail_without_blocking() {
        use std::os::unix::net::UnixListener;
        let temp = tempfile::tempdir().unwrap();
        let _socket = UnixListener::bind(temp.path().join("socket")).unwrap();
        let root = AuthorizedRoot::open(temp.path()).unwrap();
        assert!(root.open_read("socket").is_err());
        assert!(root.list("").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_links_and_cannot_escape_or_be_output_ancestors() {
        // Directory junction creation needs neither administrator privileges nor
        // Developer Mode, unlike CreateSymbolicLink on many Windows machines.
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"public fixture").unwrap();
        let link = temp.path().join("junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "creating privilege-independent junction fixture failed"
        );
        let root = AuthorizedRoot::open(temp.path()).unwrap();
        let entries = root.list("").unwrap();
        assert_eq!(entries[0].kind, "link");
        assert_eq!(entries[0].size, 0);
        assert!(root.list("junction").is_err());
        assert!(root.open_read("junction/file").is_err());
        assert!(root.exists("junction/file").is_err());
        let output = staging(temp.path());
        assert!(output.create("junction/new").is_err());
        assert!(output.edit("junction/file").is_err());
        assert!(output.mkdir("junction/sub").is_err());
        assert!(output.remove("junction/file").is_err());
        // Remove only the junction; never recursively traverse its target.
        fs::remove_dir(link).unwrap();
        assert_eq!(
            fs::read(outside.path().join("file")).unwrap(),
            b"public fixture"
        );
    }
}
