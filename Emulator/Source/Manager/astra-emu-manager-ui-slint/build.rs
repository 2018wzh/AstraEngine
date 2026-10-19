fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("material".into());
    slint_build::compile_with_config("ui/main.slint", config).expect("ASTRA_EMU_SLINT_COMPILE");
}
