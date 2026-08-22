use std::{collections::BTreeMap, env, fs, path::PathBuf};

use astra_emu_family_api::{
    LegacyFamilyPluginDescriptor, LegacyLayerTransactionV9, LegacyStepInput,
    LegacyWritableFileRequestV1,
};
use schemars::{schema::RootSchema, schema_for};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: astra-emu-family-schema <output-directory>")?;
    fs::create_dir_all(&output)?;
    let schemas: BTreeMap<&str, RootSchema> = BTreeMap::from([
        (
            "family-descriptor.schema.json",
            schema_for!(LegacyFamilyPluginDescriptor),
        ),
        (
            "legacy-layer-transaction-v9.schema.json",
            schema_for!(LegacyLayerTransactionV9),
        ),
        (
            "legacy-step-input.schema.json",
            schema_for!(LegacyStepInput),
        ),
        (
            "legacy-writable-file-v1.schema.json",
            schema_for!(LegacyWritableFileRequestV1),
        ),
    ]);
    for (name, schema) in schemas {
        let destination = output.join(name);
        let temporary = output.join(format!(".{name}.tmp"));
        let mut bytes = serde_json::to_vec_pretty(&schema)?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)?;
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(temporary, destination)?;
    }
    Ok(())
}
