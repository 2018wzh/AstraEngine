use astra_emu_musica::{
    mount_musica_with_resources, validate_musica_script, MusicaResourcePaths, ScriptEncoding,
};
use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct Sources {
    #[arg(long)]
    game_dir: PathBuf,
    #[arg(long)]
    profile: PathBuf,
    #[arg(long, default_value = "")]
    scr_archive: String,
    #[arg(long, default_value = "")]
    sys_archive: String,
    #[arg(long, default_value = "")]
    script_directory: String,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Encoding {
    Cp932,
    Gbk,
}
#[derive(Debug, Subcommand)]
pub enum Diagnostic {
    /// List stable logical archive URIs and byte sizes, without reading bodies.
    List {
        #[command(flatten)]
        sources: Sources,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 256, value_parser = clap::value_parser!(u32).range(1..=4096))]
        limit: u32,
    },
    /// Copy one bounded decoded byte range to a NEW private file; never stdout.
    Read {
        #[command(flatten)]
        sources: Sources,
        #[arg(long)]
        uri: String,
        #[arg(long)]
        offset: u64,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=16777216))]
        length: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Parse complete scripts through native include/loose/encoding resolution; no VM.
    Validate {
        #[command(flatten)]
        sources: Sources,
        #[arg(long, value_enum)]
        encoding: Encoding,
        /// Omit to validate every script in the mounted archive and loose source.
        #[arg(long)]
        uri: Option<String>,
    },
}
impl Sources {
    fn mount(&self) -> Result<astra_emu_musica::MusicaMountedVfs, Box<dyn std::error::Error>> {
        Ok(mount_musica_with_resources(
            &self.game_dir,
            &self.profile,
            &MusicaResourcePaths {
                scr_archive: self.scr_archive.clone(),
                sys_archive: self.sys_archive.clone(),
                script_directory: self.script_directory.clone(),
            },
        )?
        .0)
    }
}
pub fn run(command: Diagnostic) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Diagnostic::List {
            sources,
            offset,
            limit,
        } => {
            let vfs = sources.mount()?;
            let entries = vfs.manifest().entries.iter().skip(offset).take(limit as usize)
                .map(|entry| {
                    let physical = vfs.entry_descriptor(&entry.uri)?;
                    Ok(serde_json::json!({
                        "uri":entry.uri, "decoded_size":entry.decoded_size,
                        "media_kind":entry.media_kind, "archive_role":physical.archive_role,
                        "source_offset":physical.offset, "stored_size":physical.stored_size,
                        "aligned_size":physical.aligned_size, "unpacked_size":physical.unpacked_size,
                        "packed":physical.packed
                    }))
                }).collect::<Result<Vec<_>, astra_emu_musica::CoreError>>()?;
            println!(
                "{}",
                serde_json::to_string(
                    &serde_json::json!({"total":vfs.manifest().entries.len(),"entries":entries})
                )?
            );
        }
        Diagnostic::Read {
            sources,
            uri,
            offset,
            length,
            output,
        } => {
            let vfs = sources.mount()?;
            let bytes = vfs.read_range(&uri, offset, length)?.bytes;
            crate::private_output::write_new_private(&output, &bytes)?;
            println!("{}", serde_json::json!({"bytes":bytes.len()}));
        }
        Diagnostic::Validate {
            sources,
            encoding,
            uri,
        } => {
            let vfs = sources.mount()?;
            let encoding = match encoding {
                Encoding::Cp932 => ScriptEncoding::ShiftJis,
                Encoding::Gbk => ScriptEncoding::Gbk,
            };
            let uris = uri
                .map(|uri| vec![uri])
                .unwrap_or_else(|| vfs.script_uris());
            let mut failed = false;
            for uri in uris {
                match validate_musica_script(&vfs, &uri, encoding) {
                    Ok(report) => {
                        failed |= !report.valid;
                        println!("{}", serde_json::to_string(&report)?);
                    }
                    Err(cause) => {
                        failed = true;
                        println!(
                            "{}",
                            serde_json::json!({"uri":uri,"valid":false,"diagnostic":cause.code()})
                        );
                    }
                }
            }
            if failed {
                return Err("ASTRA_EMU_MUSICA_SCRIPT_VALIDATION".into());
            }
        }
    }
    Ok(())
}
