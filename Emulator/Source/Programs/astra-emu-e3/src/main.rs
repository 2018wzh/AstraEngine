fn main() {
    if let Err(error) = astra_emu_e3::run_from_args(std::env::args_os()) {
        eprintln!("ASTRA_EMU_E3_FAILED:{error}");
        std::process::exit(1);
    }
}
