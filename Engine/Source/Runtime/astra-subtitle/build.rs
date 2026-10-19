use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/bridge.c");
    println!("cargo:rerun-if-env-changed=ASTRA_SUBTITLE_VCPKG_ROOT");
    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let (includes, metadata) = if windows {
        assert_eq!(
            env::var("TARGET").unwrap(),
            "x86_64-pc-windows-msvc",
            "astra-subtitle requires the pinned x64 MSVC native installation on Windows"
        );
        assert!(
            !env::var("CARGO_CFG_TARGET_FEATURE")
                .unwrap_or_default()
                .split(',')
                .any(|feature| feature == "crt-static"),
            "astra-subtitle requires the MD CRT (no +crt-static)"
        );
        let root = PathBuf::from(
            env::var_os("ASTRA_SUBTITLE_VCPKG_ROOT")
                .expect("set ASTRA_SUBTITLE_VCPKG_ROOT to the isolated subtitle vcpkg root"),
        );
        let lib = vcpkg::Config::new()
            .vcpkg_root(root)
            .target_triplet("x64-windows-static-md-dynbidi")
            .cargo_metadata(false)
            .copy_dlls(false)
            .find_package("libass")
            .expect("install libass 0.17.4#1 in the isolated subtitle vcpkg root");
        assert!(lib.is_static, "libass must be static; only fribidi may be dynamic");
        for required in ["libass", "freetype", "fribidi", "harfbuzz"] {
            assert!(
                lib.ports.iter().any(|port| port == required),
                "libass installation is missing a required transitive port"
            );
        }
        (lib.include_paths, lib.cargo_metadata)
    } else {
        // A real native installation is required on other platforms too. Never
        // provide a no-op renderer for a missing native dependency.
        let lib = pkg_config::Config::new()
            .exactly_version("0.17.4")
            .probe("libass")
            .expect("astra-subtitle requires native libass exactly 0.17.4");
        // Let pkg-config emit its complete platform metadata, including
        // absolute library files, frameworks and private linker arguments.
        (lib.include_paths, Vec::new())
    };
    let mut bridge = cc::Build::new();
    bridge
        .file("src/bridge.c")
        .includes(&includes)
        .warnings(true);
    if windows {
        bridge.static_crt(false);
    }
    // bridge.c checks LIBASS_VERSION against 0x01704000 using the actual
    // installed header; it also uses the official track layout and va_list.
    bridge.compile("astra_subtitle_bridge");
    for line in metadata {
        println!("{line}");
    }
    if windows {
        // vcpkg-rs omits system libraries used by libass's desktop font
        // provider, even though provider lookup is disabled at runtime.
        println!("cargo:rustc-link-lib=gdi32");
        println!("cargo:rustc-link-lib=user32");
    }
}
