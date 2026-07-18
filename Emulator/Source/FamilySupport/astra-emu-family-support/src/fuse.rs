use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

use astra_emu_family_core::{LegacyMountedVfs, LegacyVfsNodeKind};
use fuser::{
    BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, INodeNo,
    LockOwner, MountOption, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request, TimeOrNow, WriteFlags,
};

const TTL: Duration = Duration::from_secs(1);

struct Node {
    uri: String,
    parent: u64,
    name: String,
    kind: FileType,
    size: u64,
}

struct ReadOnlyLegacyFs {
    vfs: Arc<dyn LegacyMountedVfs>,
    nodes: BTreeMap<u64, Node>,
    by_child: BTreeMap<(u64, String), u64>,
}

impl ReadOnlyLegacyFs {
    fn new(vfs: Arc<dyn LegacyMountedVfs>) -> Result<Self, Box<dyn std::error::Error>> {
        vfs.manifest().validate(10_000_000)?;
        let prefix = vfs.manifest().prefix.clone();
        let mut nodes = BTreeMap::from([(
            1,
            Node {
                uri: prefix.clone(),
                parent: 1,
                name: "/".into(),
                kind: FileType::Directory,
                size: 0,
            },
        )]);
        let mut by_child = BTreeMap::new();
        let mut by_uri = BTreeMap::from([(prefix.clone(), 1u64)]);
        let mut next = 2u64;
        for entry in &vfs.manifest().entries {
            let relative = entry
                .uri
                .strip_prefix(&prefix)
                .ok_or("ASTRA_EMU_VFS_FUSE_URI")?;
            let parts = relative.split('/').collect::<Vec<_>>();
            if parts.iter().any(|part| part.is_empty()) {
                return Err("ASTRA_EMU_VFS_FUSE_URI".into());
            }
            let mut parent = 1;
            let mut current = prefix.clone();
            for (index, part) in parts.iter().enumerate() {
                current.push_str(part);
                let last = index + 1 == parts.len();
                let ino = if let Some(ino) = by_uri.get(&current) {
                    *ino
                } else {
                    let ino = next;
                    next = next.checked_add(1).ok_or("ASTRA_EMU_VFS_FUSE_INODE")?;
                    nodes.insert(
                        ino,
                        Node {
                            uri: current.clone(),
                            parent,
                            name: (*part).into(),
                            kind: if last {
                                FileType::RegularFile
                            } else {
                                FileType::Directory
                            },
                            size: if last { entry.decoded_size } else { 0 },
                        },
                    );
                    by_uri.insert(current.clone(), ino);
                    if by_child.insert((parent, (*part).into()), ino).is_some() {
                        return Err("ASTRA_EMU_VFS_FUSE_DUPLICATE".into());
                    }
                    ino
                };
                parent = ino;
                if !last {
                    current.push('/');
                }
            }
        }
        Ok(Self {
            vfs,
            nodes,
            by_child,
        })
    }

    fn attr(&self, ino: u64) -> Option<FileAttr> {
        self.nodes.get(&ino).map(|node| FileAttr {
            ino: INodeNo(ino),
            size: node.size,
            blocks: node.size.div_ceil(512),
            atime: SystemTime::UNIX_EPOCH,
            mtime: SystemTime::UNIX_EPOCH,
            ctime: SystemTime::UNIX_EPOCH,
            crtime: SystemTime::UNIX_EPOCH,
            kind: node.kind,
            perm: if node.kind == FileType::Directory {
                0o555
            } else {
                0o444
            },
            nlink: 1,
            uid: unsafe { libc::geteuid() },
            gid: unsafe { libc::getegid() },
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }
}

impl Filesystem for ReadOnlyLegacyFs {
    fn lookup(&self, _: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            return reply.error(Errno::ENOENT);
        };
        match self
            .by_child
            .get(&(parent.0, name.into()))
            .and_then(|ino| self.attr(*ino))
        {
            Some(attr) => reply.entry(&TTL, &attr, 0),
            None => reply.error(Errno::ENOENT),
        }
    }
    fn getattr(&self, _: &Request, ino: INodeNo, _: Option<FileHandle>, reply: ReplyAttr) {
        match self.attr(ino.0) {
            Some(attr) => reply.attr(&TTL, &attr),
            None => reply.error(Errno::ENOENT),
        }
    }
    fn setattr(
        &self,
        _: &Request,
        _: INodeNo,
        _: Option<u32>,
        _: Option<u32>,
        _: Option<u32>,
        _: Option<u64>,
        _: Option<TimeOrNow>,
        _: Option<TimeOrNow>,
        _: Option<SystemTime>,
        _: Option<FileHandle>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        reply.error(Errno::EROFS)
    }
    fn mknod(&self, _: &Request, _: INodeNo, _: &OsStr, _: u32, _: u32, _: u32, reply: ReplyEntry) {
        reply.error(Errno::EROFS)
    }
    fn mkdir(&self, _: &Request, _: INodeNo, _: &OsStr, _: u32, _: u32, reply: ReplyEntry) {
        reply.error(Errno::EROFS)
    }
    fn unlink(&self, _: &Request, _: INodeNo, _: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS)
    }
    fn rmdir(&self, _: &Request, _: INodeNo, _: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS)
    }
    fn symlink(&self, _: &Request, _: INodeNo, _: &OsStr, _: &Path, reply: ReplyEntry) {
        reply.error(Errno::EROFS)
    }
    fn rename(
        &self,
        _: &Request,
        _: INodeNo,
        _: &OsStr,
        _: INodeNo,
        _: &OsStr,
        _: RenameFlags,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS)
    }
    fn link(&self, _: &Request, _: INodeNo, _: INodeNo, _: &OsStr, reply: ReplyEntry) {
        reply.error(Errno::EROFS)
    }
    fn open(&self, _: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if flags.0 & (libc::O_WRONLY | libc::O_RDWR | libc::O_TRUNC | libc::O_CREAT) != 0 {
            return reply.error(Errno::EROFS);
        }
        if self
            .nodes
            .get(&ino.0)
            .is_none_or(|node| node.kind != FileType::RegularFile)
        {
            return reply.error(Errno::ENOENT);
        }
        reply.opened(FileHandle(ino.0), FopenFlags::empty())
    }
    fn read(
        &self,
        _: &Request,
        ino: INodeNo,
        _: FileHandle,
        offset: u64,
        size: u32,
        _: OpenFlags,
        _: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let Some(node) = self.nodes.get(&ino.0) else {
            return reply.error(Errno::ENOENT);
        };
        let length = (size as u64).min(node.size.saturating_sub(offset));
        match self.vfs.read_range(&node.uri, offset, length) {
            Ok(read) => reply.data(&read.bytes),
            Err(_) => reply.error(Errno::EIO),
        }
    }
    fn readdir(
        &self,
        _: &Request,
        ino: INodeNo,
        _: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        if self
            .nodes
            .get(&ino.0)
            .is_none_or(|node| node.kind != FileType::Directory)
        {
            return reply.error(Errno::ENOTDIR);
        }
        let mut entries = vec![
            (ino.0, FileType::Directory, ".".to_owned()),
            (
                self.nodes[&ino.0].parent,
                FileType::Directory,
                "..".to_owned(),
            ),
        ];
        for ((parent, _), child) in self
            .by_child
            .range((ino.0, String::new())..=(ino.0, String::from("\u{10ffff}")))
        {
            if *parent == ino.0 {
                let node = &self.nodes[child];
                entries.push((*child, node.kind, node.name.clone()));
            }
        }
        for (index, (entry, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(INodeNo(entry), (index + 1) as u64, kind, name) {
                break;
            }
        }
        reply.ok()
    }
    fn write(
        &self,
        _: &Request,
        _: INodeNo,
        _: FileHandle,
        _: u64,
        _: &[u8],
        _: WriteFlags,
        _: OpenFlags,
        _: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::EROFS)
    }
    fn create(
        &self,
        _: &Request,
        _: INodeNo,
        _: &OsStr,
        _: u32,
        _: u32,
        _: i32,
        reply: ReplyCreate,
    ) {
        reply.error(Errno::EROFS)
    }
}

pub fn mount_read_only(
    vfs: Arc<dyn LegacyMountedVfs>,
    mountpoint: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !mountpoint.is_dir() {
        return Err("ASTRA_EMU_VFS_FUSE_MOUNTPOINT".into());
    }
    let family_id = vfs.manifest().family_id.clone();
    let config = Config {
        mount_options: vec![
            MountOption::RO,
            MountOption::NoDev,
            MountOption::NoSuid,
            MountOption::NoExec,
            MountOption::DefaultPermissions,
            MountOption::FSName(format!("astraemu-{family_id}")),
        ],
        ..Config::default()
    };
    fuser::mount2(ReadOnlyLegacyFs::new(vfs)?, mountpoint, &config)?;
    Ok(())
}
