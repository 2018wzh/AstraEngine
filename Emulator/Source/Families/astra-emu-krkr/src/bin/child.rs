fn main() {
    let result = astra_emu_krkr::child::connection_arguments().and_then(|(session, port)| {
        astra_emu_krkr::child::serve(session, port, astra_emu_krkr::child::DllOwner::default())
    });
    if result.is_err() {
        // Native and transport failures are carried by the private protocol.
        std::process::exit(1);
    }
}
