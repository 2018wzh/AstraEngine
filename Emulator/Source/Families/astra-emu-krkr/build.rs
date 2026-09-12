//! Builds the vendored Kirikiri core as a static library through CMake and
//! links it into the family cdylib.
//!
//! Requirements (documented in MODIFICATIONS.md):
//! - CMake >= 3.28 and a VS x64 environment (`cl` on PATH) when invoked
//!   outside a developer prompt, run from "x64 Native Tools Command Prompt".
//! - `VCPKG_ROOT` pointing at a vcpkg checkout that contains the baseline
//!   recorded in `ThirdParty/kirikiri2/vcpkg-configuration.json`.
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
    run(Command::new("cmake")
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
        .arg("-DVCPKG_TARGET_TRIPLET=x64-windows")
        .arg("-DVCPKG_BUILD_TYPE=release"));
    run(Command::new("cmake")
        .arg("--build")
        .arg(build_dir.display().to_string())
        .arg("--target")
        .arg("astra-krkr-hosted"));

    println!(
        "cargo:rustc-link-search=native={}",
        build_dir.join("lib").display()
    );
    link_prebuilt();
}

fn link_prebuilt() {
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
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to spawn {:?}: {error}", command));
    if !status.success() {
        panic!("command failed with {status}: {:?}", command);
    }
}
