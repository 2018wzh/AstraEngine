use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _observability = astra_observability::init_host(
        astra_observability::HostObservabilityConfig::for_cli("info"),
    )?;
    tracing::info!(event = "astra.emu.evidence_encoder.started");
    let result = run();
    match &result {
        Ok(()) => tracing::info!(event = "astra.emu.evidence_encoder.completed"),
        Err(_) => tracing::error!(
            event = "astra.emu.evidence_encoder.failed",
            diagnostic_code = "ASTRA_EMU_EVIDENCE_ENCODER_FAILED"
        ),
    }
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().map(PathBuf::from);
    let project_root = args.next().map(PathBuf::from);
    let output = args.next().map(PathBuf::from);
    let target = args.next().and_then(|value| value.into_string().ok());
    let profile = args.next().and_then(|value| value.into_string().ok());
    if args.next().is_some()
        || input.is_none()
        || project_root.is_none()
        || output.is_none()
        || target.is_none()
        || profile.is_none()
    {
        return Err("usage: astra-emu-evidence <input-directory> <project-root> <project-relative-output-directory> <target> <profile>".into());
    }
    astra_emu_evidence::encode_bundle(
        &input.unwrap(),
        &project_root.unwrap(),
        &output.unwrap(),
        &target.unwrap(),
        &profile.unwrap(),
    )?;
    Ok(())
}
