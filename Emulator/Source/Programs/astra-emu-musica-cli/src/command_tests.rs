use super::*;

#[test]
fn converters_require_missing_algorithm_metadata_explicitly() {
    let garbro = [
        "astra-emu-musica-cli",
        "import-garbro-scheme",
        "--formats",
        "Formats.dat",
        "--title",
        "Public Fixture",
        "--output",
        "key.toml",
    ];
    assert!(Cli::try_parse_from(garbro).is_err());
    let mut args = garbro.to_vec();
    args.extend(["--index-size-xor", "0"]);
    assert!(Cli::try_parse_from(&args).is_err());
    args.extend(["--rc4-skip-crc", "false"]);
    let cli = Cli::try_parse_from(&args).unwrap();
    assert!(matches!(
        cli.command,
        Command::ImportGarbroScheme {
            index_size_xor: 0,
            rc4_skip_crc: false,
            ..
        }
    ));
    args.extend(["--paz-version", "2"]);
    assert!(Cli::try_parse_from(&args).is_err());

    let native = [
        "astra-emu-musica-cli",
        "import-key-profile",
        "--file",
        "legacy.toml",
        "--output",
        "key.toml",
    ];
    assert!(Cli::try_parse_from(native).is_err());
    let mut args = native.to_vec();
    args.extend(["--index-size-xor", "0", "--rc4-skip-crc", "true"]);
    assert!(Cli::try_parse_from(&args).is_err());
    args.extend(["--paz-version", "2"]);
    assert!(matches!(
        Cli::try_parse_from(&args).unwrap().command,
        Command::ImportKeyProfile {
            paz_version: 2,
            index_size_xor: 0,
            rc4_skip_crc: true,
            ..
        }
    ));
}

#[test]
fn native_data_diagnostics_require_explicit_encoding_and_bounded_private_output() {
    let prefix = [
        "astra-emu-musica-cli",
        "data",
        "validate",
        "--game-dir",
        "public",
        "--profile",
        "key.toml",
    ];
    assert!(Cli::try_parse_from(prefix).is_err());
    let mut args = prefix.to_vec();
    args.extend([
        "--encoding",
        "gbk",
        "--scr-archive",
        "cnscr.paz",
        "--script-directory",
        "scripts",
    ]);
    assert!(Cli::try_parse_from(args).is_ok());
    let read = [
        "astra-emu-musica-cli",
        "data",
        "read",
        "--game-dir",
        "public",
        "--profile",
        "key.toml",
        "--uri",
        "musica:/scr/public.sc",
        "--offset",
        "0",
        "--length",
        "16777217",
        "--output",
        "private.bin",
    ];
    assert!(Cli::try_parse_from(read).is_err());
}
