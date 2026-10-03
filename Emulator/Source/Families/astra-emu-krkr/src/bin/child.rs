fn main() {
    let result = astra_emu_krkr::child::connection_arguments().and_then(|(session, port)| {
        astra_emu_krkr::child::serve(session, port, astra_emu_krkr::child::UnavailableOwner)
    });
    if result.is_err() {
        // No native owner is linked yet. The private protocol carries failure.
        std::process::exit(1);
    }
}
