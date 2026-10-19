use std::{collections::HashMap, path::PathBuf};

fn main() {
    let material = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("material-1.1.0/material.slint");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("material".into())
        .with_library_paths(HashMap::from([("material".to_string(), material)]));
    slint_build::compile_with_config("ui/main.slint", config).expect("ASTRA_EMU_SLINT_COMPILE");
}
