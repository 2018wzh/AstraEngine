use std::{env, fs, path::PathBuf};

use astra_emu_extension_api::{TranslationTextRequestV1, TranslationTextResponseV1};
use schemars::{schema::RootSchema, schema_for};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: astra-emu-extension-schema <output-directory>")?;
    fs::create_dir_all(&output)?;
    for (name, schema) in [
        (
            "translation-text-request-v1.schema.json",
            schema_for!(TranslationTextRequestV1),
        ),
        (
            "translation-text-response-v1.schema.json",
            schema_for!(TranslationTextResponseV1),
        ),
    ] {
        write_schema(&output, name, &schema)?;
    }
    Ok(())
}

fn write_schema(
    output: &std::path::Path,
    name: &str,
    schema: &RootSchema,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = output.join(name);
    let temporary = output.join(format!(".{name}.tmp"));
    let mut bytes = serde_json::to_vec_pretty(schema)?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes)?;
    if destination.exists() {
        fs::remove_file(&destination)?;
    }
    fs::rename(temporary, destination)?;
    Ok(())
}
