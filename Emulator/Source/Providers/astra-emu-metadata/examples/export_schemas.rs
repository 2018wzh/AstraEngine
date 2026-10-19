use std::path::PathBuf;

use astra_emu_metadata::{CompatibilityDatabase, CoreCatalog, MetadataIndex, PresetDatabase};
use schemars::schema::RootSchema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(output), None) = (args.next(), args.next()) else {
        return Err("usage: export_schemas <output-directory>".into());
    };
    let output = PathBuf::from(output);
    std::fs::create_dir_all(&output)?;
    let schemas: [(&str, RootSchema); 4] = [
        (
            "compatibility.v3.schema.json",
            schemars::schema_for!(CompatibilityDatabase),
        ),
        ("index.v1.schema.json", schemars::schema_for!(MetadataIndex)),
        (
            "presets.v1.schema.json",
            schemars::schema_for!(PresetDatabase),
        ),
        ("cores.v1.schema.json", schemars::schema_for!(CoreCatalog)),
    ];
    for (name, schema) in schemas {
        let mut text = serde_json::to_string_pretty(&schema)?;
        text.push('\n');
        std::fs::write(output.join(name), text)?;
    }
    Ok(())
}
