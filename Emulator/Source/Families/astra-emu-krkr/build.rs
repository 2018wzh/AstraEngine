fn main() {
    println!("cargo:rerun-if-env-changed=ASTRA_KRKR_NATIVE_LIB_DIR");
    let target = std::env::var("TARGET").expect("Cargo TARGET");
    assert_eq!(target, "x86_64-pc-windows-msvc", "KrKr native currently requires Windows x64/MSVC");
    let directory = std::env::var_os("ASTRA_KRKR_NATIVE_LIB_DIR")
        .expect("Build astra_krkr_native first; set ASTRA_KRKR_NATIVE_LIB_DIR to its owned output directory");
    let directory = std::path::PathBuf::from(directory);
    assert!(directory.join("astra_emu_krkr.lib").is_file(), "Missing astra_emu_krkr.lib");
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=dylib=astra_emu_krkr");
}
