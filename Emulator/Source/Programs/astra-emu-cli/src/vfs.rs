use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use astra_core::Hash256;
use astra_emu_family_core::{LegacyCoreError, LegacyMountedVfs, LEGACY_VFS_MAX_READ_BYTES};
use astra_emu_family_support::{
    enforce_private_file_permissions, extract_vfs, verify_vfs, ExtractSelection,
    LegacyVfsFamilyRegistry,
};
use astra_emu_minori::MinoriVfsFamilyFactory;
use clap::{Args, Subcommand, ValueEnum};
use encoding_rs::Encoding;
use serde::Serialize;

const STDOUT_PAYLOAD_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Args)]
pub struct VfsArgs {
    #[arg(long)]
    family: String,
    #[arg(long)]
    game_dir: PathBuf,
    #[arg(long)]
    mount_profile: PathBuf,
    #[command(subcommand)]
    command: VfsCommand,
}

#[derive(Debug, Subcommand)]
enum VfsCommand {
    Verify,
    Extract {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, conflicts_with_all = ["glob", "entry"])]
        prefix: Option<String>,
        #[arg(long, conflicts_with_all = ["prefix", "entry"])]
        glob: Option<String>,
        #[arg(long, conflicts_with_all = ["prefix", "glob"])]
        entry: Option<String>,
    },
    List {
        #[arg(long)]
        uri: String,
    },
    Stat {
        #[arg(long)]
        uri: String,
    },
    Read {
        #[arg(long)]
        uri: String,
        #[arg(long)]
        offset: u64,
        #[arg(long)]
        length: u64,
        #[arg(long, conflicts_with = "output")]
        format: Option<ReadFormat>,
        #[arg(long, requires = "format")]
        encoding: Option<String>,
        #[arg(long, conflicts_with_all = ["format", "encoding"])]
        output: Option<PathBuf>,
    },
    #[cfg(target_os = "linux")]
    Mount {
        #[arg(long)]
        mountpoint: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReadFormat {
    Hex,
    Text,
}

#[derive(Debug, Serialize)]
struct ReadEvidence {
    schema: &'static str,
    family_id: String,
    offset: u64,
    length: u64,
    hash: Hash256,
    eof: bool,
    cache_hit: bool,
}

pub fn run(arguments: VfsArgs) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = mount(&arguments)?;
    match arguments.command {
        VfsCommand::Verify => println!("{}", serde_json::to_string(&verify_vfs(vfs.as_ref())?)?),
        VfsCommand::Extract {
            output,
            prefix,
            glob,
            entry,
        } => {
            let report = extract_vfs(
                vfs.as_ref(),
                &output,
                &ExtractSelection {
                    prefix,
                    glob,
                    entry,
                },
                &AtomicBool::new(false),
            )?;
            println!("{}", serde_json::to_string(&report)?);
        }
        VfsCommand::List { uri } => {
            #[derive(Serialize)]
            struct Output<'a> {
                schema: &'static str,
                family_id: &'a str,
                nodes: Vec<astra_emu_family_core::LegacyVfsNode>,
            }
            let nodes = vfs.read_dir(&uri)?;
            println!(
                "{}",
                serde_json::to_string(&Output {
                    schema: "astra.emu.vfs.list.v1",
                    family_id: &vfs.manifest().family_id,
                    nodes
                })?
            );
        }
        VfsCommand::Stat { uri } => {
            #[derive(Serialize)]
            struct Output<'a> {
                schema: &'static str,
                family_id: &'a str,
                stat: astra_emu_family_core::LegacyVfsStat,
            }
            let stat = vfs.stat(&uri)?;
            println!(
                "{}",
                serde_json::to_string(&Output {
                    schema: "astra.emu.vfs.stat.v1",
                    family_id: &vfs.manifest().family_id,
                    stat
                })?
            );
        }
        VfsCommand::Read {
            uri,
            offset,
            length,
            format,
            encoding,
            output,
        } => read(
            vfs.as_ref(),
            &uri,
            offset,
            length,
            format,
            encoding.as_deref(),
            output.as_deref(),
        )?,
        #[cfg(target_os = "linux")]
        VfsCommand::Mount { mountpoint } => {
            astra_emu_family_support::mount_read_only(vfs, &mountpoint)?
        }
    }
    Ok(())
}

fn mount(arguments: &VfsArgs) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
    let mut registry = LegacyVfsFamilyRegistry::default();
    registry.register(Arc::new(MinoriVfsFamilyFactory))?;
    let loaded = registry.load_profile(&arguments.mount_profile)?;
    registry.mount(&arguments.family, &arguments.game_dir, &loaded)
}

fn read(
    vfs: &dyn LegacyMountedVfs,
    uri: &str,
    offset: u64,
    length: u64,
    format: Option<ReadFormat>,
    encoding: Option<&str>,
    output: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    if length == 0 || length > LEGACY_VFS_MAX_READ_BYTES {
        return Err("ASTRA_EMU_VFS_READ_LIMIT".into());
    }
    if matches!(format, Some(ReadFormat::Hex)) && encoding.is_some() {
        return Err("ASTRA_EMU_VFS_READ_ENCODING_CONFLICT".into());
    }
    if matches!(format, Some(ReadFormat::Text)) && encoding.is_none() {
        return Err("ASTRA_EMU_VFS_READ_ENCODING_REQUIRED".into());
    }
    if format.is_some() && length > STDOUT_PAYLOAD_LIMIT {
        return Err("ASTRA_EMU_VFS_READ_STDOUT_LIMIT".into());
    }
    let read = vfs.read_range(uri, offset, length)?;
    if read.bytes.len() as u64 != length {
        return Err("ASTRA_EMU_VFS_READ_SHORT".into());
    }
    if let Some(path) = output {
        write_private(path, &read.bytes)?;
        return Ok(());
    }
    match format {
        Some(ReadFormat::Hex) => println!(
            "{}",
            read.bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        Some(ReadFormat::Text) => {
            let label = encoding.expect("validated text encoding");
            let codec =
                Encoding::for_label(label.as_bytes()).ok_or("ASTRA_EMU_VFS_READ_ENCODING")?;
            let (text, _, malformed) = codec.decode(&read.bytes);
            if malformed {
                return Err("ASTRA_EMU_VFS_READ_TEXT_DECODE".into());
            }
            print!("{text}");
        }
        None => println!(
            "{}",
            serde_json::to_string(&ReadEvidence {
                schema: "astra.emu.vfs.read.v1",
                family_id: vfs.manifest().family_id.clone(),
                offset,
                length,
                hash: Hash256::from_sha256(&read.bytes),
                eof: read.eof,
                cache_hit: read.cache_hit
            })?
        ),
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("ASTRA_EMU_VFS_READ_OUTPUT_EXISTS".into());
    }
    let parent = path
        .parent()
        .filter(|path| path.is_dir())
        .ok_or("ASTRA_EMU_VFS_READ_OUTPUT_PARENT")?;
    let name = path
        .file_name()
        .ok_or("ASTRA_EMU_VFS_READ_OUTPUT_NAME")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.astra-tmp"));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    if let Err(error) = enforce_private_file_permissions(&temporary) {
        drop(file);
        if fs::remove_file(&temporary).is_err() {
            return Err("ASTRA_EMU_VFS_READ_OUTPUT_PERMISSION_CLEANUP".into());
        }
        return Err(error.into());
    }
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}
