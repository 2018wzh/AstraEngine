use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: astra-emu-schema <output-directory>")?;
    astra_emu_schema::generate(&output)
}
