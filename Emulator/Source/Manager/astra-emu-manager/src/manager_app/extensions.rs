use super::*;
pub(super) fn run(command: crate::ExtensionCommand) -> Result<(), String> {
    let directory = platform_data_dir().map_err(|e| e.to_string())?;
    let store = astra_emu_manager_core::extensions::ExtensionStore::new(&directory);
    match command {
        crate::ExtensionCommand::Install { id, source } => store.install(&id, &source),
        crate::ExtensionCommand::Enable { id, enabled } => {
            store.configure(&id, Some(enabled), None)
        }
        crate::ExtensionCommand::Configure { id, configuration } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            fs::File::open(configuration)
                .map_err(|e| e.to_string())?
                .take(16385)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 16384 {
                return Err("ASTRA_EMU_EXTENSION_CONFIG".into());
            }
            store.configure(
                &id,
                None,
                Some(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?),
            )
        }
        crate::ExtensionCommand::List => {
            for extension in store.list()? {
                println!(
                    "{}\t{}",
                    extension.id,
                    if extension.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            Ok(())
        }
    }
}
