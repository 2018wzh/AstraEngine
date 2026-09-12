//! Builds the vendored Kirikiri core as a shared library through CMake and
//! links it into the family cdylib.
//!
//! Requirements (documented in MODIFICATIONS.md):
//! - CMake >= 3.28 and Ninja.
//! - `VCPKG_ROOT` pointing at a vcpkg checkout that contains the baseline
//!   recorded in `ThirdParty/kirikiri2/vcpkg-configuration.json`.
//! - Windows additionally needs a VS x64 environment (`cl` on PATH).
//! - `ASTRA_KRKR_BINARY_CACHE` (optional) redirects the vcpkg binary cache.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let engine = env::var("CARGO_FEATURE_ENGINE").is_ok();
    if !engine {
        println!(
            "cargo:warning=astra-emu-krkr built without the `engine` feature; engine FFI symbols are unresolved"
        );
        return;
    }
    if env::var("ASTRA_KRKR_SKIP_ENGINE_BUILD").is_ok() {
        link_prebuilt();
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let core_dir = manifest_dir
        .join("..")
        .join("..")
        .join("..")
        .join("ThirdParty")
        .join("kirikiri2")
        .canonicalize()
        .expect("vendored kirikiri2 core directory exists");
    let build_dir = env::var("ASTRA_KRKR_BUILD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| core_dir.join("build").join("astra-hosted"));

    let vcpkg_root = env::var("VCPKG_ROOT")
        .expect("VCPKG_ROOT must point at a vcpkg checkout matching the vendored baseline");

    let _ = std::fs::create_dir_all(&build_dir);
    let mut configure = Command::new("cmake");
    configure
        .arg(format!("-S{}", core_dir.display()))
        .arg(format!("-B{}", build_dir.display()))
        .arg("-GNinja")
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DKRKR2_ASTRA_HOSTED=ON")
        .arg("-DENABLE_TESTS=OFF")
        .arg("-DBUILD_TOOLS=OFF")
        .arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={vcpkg_root}/scripts/buildsystems/vcpkg.cmake"
        ))
        // The hosted build consumes the manifest's `astra-hosted` feature,
        // not the cocos2d `engine-full` default.
        .arg("-DVCPKG_MANIFEST_FEATURES=astra-hosted")
        .arg("-DVCPKG_MANIFEST_NO_DEFAULT_FEATURES=ON")
        .arg(format!("-DVCPKG_TARGET_TRIPLET={}", host_triplet()))
        .arg("-DVCPKG_BUILD_TYPE=release");
    if !cfg!(target_os = "windows") {
        // Pin the host compiler so the engine and the vcpkg dependencies are
        // built with the same toolchain. CC/CXX override the default.
        let cc = env::var("CC").unwrap_or_else(|_| "gcc".to_string());
        let cxx = env::var("CXX").unwrap_or_else(|_| "g++".to_string());
        configure
            .arg(format!("-DCMAKE_C_COMPILER={cc}"))
            .arg(format!("-DCMAKE_CXX_COMPILER={cxx}"));
    }
    run(&mut configure);
    run(Command::new("cmake")
        .arg("--build")
        .arg(build_dir.display().to_string())
        .arg("--target")
        .arg("astra-krkr-hosted"));

    link_prebuilt();
}

fn host_triplet() -> &'static str {
    if cfg!(target_os = "windows") {
        "x64-windows"
    } else {
        "x64-linux"
    }
}

fn link_prebuilt() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let build_dir = env::var("ASTRA_KRKR_BUILD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            manifest_dir
                .join("..")
                .join("..")
                .join("..")
                .join("ThirdParty")
                .join("kirikiri2")
                .join("build")
                .join("astra-hosted")
        });
    if cfg!(target_os = "windows") {
        // The CMake target is SHARED: link its import lib and rely on the
        // DLL sitting next to the host binary or on PATH.
        println!(
            "cargo:rustc-link-search=native={}",
            build_dir.join("lib").display()
        );
        println!("cargo:rustc-link-lib=static=astra-krkr-hosted");
        // MSVC runtimes and Win32 libraries the core pulls in.
        println!("cargo:rustc-link-lib=dylib=user32");
        println!("cargo:rustc-link-lib=dylib=gdi32");
        println!("cargo:rustc-link-lib=dylib=shell32");
        println!("cargo:rustc-link-lib=dylib=ole32");
        println!("cargo:rustc-link-lib=dylib=oleaut32");
        println!("cargo:rustc-link-lib=dylib=uuid");
        println!("cargo:rustc-link-lib=dylib=advapi32");
        println!("cargo:rustc-link-lib=dylib=ws2_32");
        println!("cargo:rustc-link-lib=dylib=winmm");
        println!("cargo:rustc-link-lib=dylib=imm32");
        println!("cargo:rustc-link-lib=dylib=shlwapi");
        println!("cargo:rustc-link-lib=dylib=version");
    } else {
        // The .so lands in LIBRARY_OUTPUT_DIRECTORY (lib/) and carries the
        // vendored static dependencies inside. Link it directly and embed an
        // rpath so test binaries and the family cdylib resolve it.
        let lib_dir = build_dir.join("lib");
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=dylib=astra-krkr-hosted");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    }
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to spawn {:?}: {error}", command));
    if !status.success() {
        panic!("command failed with {status}: {:?}", command);
    }
}
