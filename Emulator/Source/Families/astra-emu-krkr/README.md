# KrKr native core

The module exports the unified C `astra_emu_query` table. `astra_emu_krkr_query`
is its uniquely named linked entry; Rust exposes that function as `query` and
`QUERY`. The native owner schedules the original VM and Cocos/OpenGL renderer.
Manager presents CPU leases and supplies optional typed window/modal services.
There is no child process, socket protocol, Host PCM requirement, library reload,
or second application event loop. Audio uses the original OpenAL playback device.

## Source provenance

- Full retained repository: `fenghengzhi/kirikiroid2-web`, base
  `7105b50822097d11b7b1f5894bf541509fd51350`.
- Owned fork: `https://github.com/2018wzh/kirikiroid2-web`, pinned at
  `d3163b1a08a622f95f916c5bc896f71532578790`.
  The retained Windows patch is separate commit
  `cae1bd8b98a817505f5a973ac00e6b925fbe35e3`; lifecycle fixes follow it.
- `layerExImage`: `23a9214f9b6e9bc3267b964707efa269c041b919`, compiled unchanged.
- The retained tracked Windows/native patch and all five untracked source/overlay
  files were copied from the read-only prior checkout before lifecycle changes.
  The eleven-file retained native seam is the origin of `native/`; `HostAudio.h`
  and the private process ABI exports were removed by this cutover.
- `native/include/astra_emu.h` is the generated shared header at AstraEngine
  `15a39a626b1316cb6137a75e17e14a2ff2d1ba88` (ABI v2). Regenerate/copy it with the shared
  crate whenever that ABI revision changes; do not edit the header independently.

## Ownership and reopen

Only one original VM may run per loaded module. Each successful open owns a fresh
native **thread**, application, VM, original worker set and GL context in the same
process. Close joins that thread; the next open uses the same resident DLL.
Renderer/uniform lookup caches are owner-thread-local and expire when that owner
joins. The general core changes retain static shutdown registrations, reset VM,
plugin registrations, options, storage caches, fonts, glyph textures, logging,
scene pointers and per-context render resources. Original `tTVPThread` workers
are joinable; termination and suspended-start wakeup are synchronized. The image
loader and sound/timer/video producers stop before VM teardown. A failed cleanup
poisons further opens rather than reusing partially destroyed globals.

The cancellation callback exists before synchronous open returns. VM bytecode
checks its owner cancellation every 1024 instructions using a silent exception
which bypasses script try/catch. Cancel invalidates host services independently
of the owner queue. Close returns after callbacks and workers finish. Partial
startup uses the same cleanup path. Native calls that perform blocking external
I/O must return before their owning worker can join; cancellation is cooperative.

Three CPU slots bound frame storage. A published slot is immutable while leased;
when all reusable slots are leased the native canvas stays dirty until a slot is
returned. Lease release owns only its CPU pool, never the closed VM, GL context,
Host callbacks or session. Keep the DLL resident until every lease is released.
Input uses the displayed frame identity and converts raster pixels to authored
stage coordinates once. Stale identities fail explicitly. Horizontal wheel is
unsupported; vertical wheel preserves fractional line detents.

`startup.tjs` directories and conventional `data.xp3` directories use the
original selector entry rule. Only Cocos UI/cache files use a session-owned
writable directory; cleanup removes only that directory. Original
`ApplicationSpecialPath` game `savedata` and `Kirikiroid2Preference.xml` locations
are unchanged. Never run acceptance against the sole commercial save copy.
Only compiled original plugins are registered; arbitrary legacy TPM/DLL loading
is not introduced. Unsupported platform requests fail instead of inventing a
modal answer. No text-hook capability is advertised.

## Build prerequisites

Windows x64, MSVC 14.51 (or a compatible verified toolset), CMake >=3.28, Ninja,
WinFlexBison 2.5.25/Bison 3.8.2, native OpenGL and an OpenAL playback device.
Use the retained original vcpkg manifest/overlays: static FFmpeg 3.3.9 with Yasm,
Cocos 3.17.2, SevenZip 26.04, UnRAR and all original plugins. Do not substitute
SDK FFmpeg, skip layerExImage, or use the Artemis renderer.

Run from the exclusive checkout in a captured `vcvars64.bat` environment. Set
`VCPKG_ROOT` to the tool checkout and `BISON_EXECUTABLE` to the installed
WinFlexBison binary. All generated/install/cache directories below belong to
this checkout, never the retained source tree:

```powershell
$root = (Get-Location).Path
$native = "$root/Emulator/Source/Families/astra-emu-krkr/native"
cmake -S Emulator/ThirdParty/kirikiri2 -B .work/krkr-msvc -G Ninja `
  -DWINDOWS=ON -DCMAKE_BUILD_TYPE=Release -DENABLE_TESTS=OFF -DBUILD_TOOLS=OFF `
  -DKRKR2_USE_STUB_SPDLOG=OFF -DKRKR2_EMBEDDED_ADAPTER_DIR="$native" `
  -DCMAKE_C_COMPILER="$env:VCToolsInstallDir/bin/Hostx64/x64/cl.exe" `
  -DCMAKE_CXX_COMPILER="$env:VCToolsInstallDir/bin/Hostx64/x64/cl.exe" `
  -DBISON_EXECUTABLE="$env:BISON_EXECUTABLE" -DVCPKG_TARGET_TRIPLET=x64-windows `
  -DVCPKG_INSTALLED_DIR="$root/.work/krkr-native/vcpkg_installed" `
  "-DVCPKG_INSTALL_OPTIONS=--x-buildtrees-root=$root/.work/krkr-buildtrees;--x-packages-root=$root/.work/krkr-packages;--downloads-root=$root/.work/krkr-downloads" `
  -DASTRA_KRKR_BUILD_SMOKE=ON
cmake --build .work/krkr-msvc --target astra_krkr_native astra_krkr_lifecycle_smoke
```

The native target copies app-local dependencies and `Resources/`. Supply the
matching MSVC C++/OpenMP runtime DLLs beside `astra_emu_krkr.dll` (or install
that runtime). For linked Manager builds set `ASTRA_KRKR_NATIVE_LIB_DIR` to the
directory containing `astra_emu_krkr.lib`; no child executable is packaged.
The native DLL itself is the dynamic core module.

```powershell
python Tools/AstraEMU/package_krkr_runtime.py --native-runtime "$nativeRuntime" --output "$runtimeBundle"
```

`$nativeRuntime` is the actual new native output directory, not a renamed legacy
DLL. Add `--runtime-file <DLL>` for app-local redistributables when needed.

## Verified acceptance

On 2026-10-18, the Windows x64 Release native targets built with MSVC 14.51,
CMake/Ninja and an exclusively copied retained dependency install
(`VCPKG_MANIFEST_INSTALL=OFF`). The Rust linked-query test passed against the
real DLL and shared ABI v2.

Copy `tests/fixtures/native_lifecycle/` into an owned writable test directory,
then run from the staged runtime:

```powershell
astra_krkr_lifecycle_smoke <absolute astra_emu_krkr.dll> <public-fixture-copy> --public-fixture
```

The harness loads the DLL exactly once. It verified:

- Original NVIDIA RTX 4070 Laptop OpenGL canvas: raster/stage 320x240, stride 2048.
- RGBA background `(32,64,96,255)`, rectangle `(224,32,64,255)`, PNG alpha result
  `(16,160,48,255)` and 784 rendered glyph pixels. Initial frame hash:
  `2466111679093113121`.
- Static `Unchanged`, four real key-driven frame changes per ordinary session,
  and the first immutable CPU lease remaining valid through changes and closes.
- Native looping WAV/OpenAL playback while closing; all owner joins returned.
- Same-module close/reopen, real `System.exit(0)` reaching `Finished`.
- Host cancellation of an infinite startup TJS loop before open returns:
  `CANCELLED` with null session, followed by another successful native open.

Three successful native opens and one cancelled open completed in that one
process/module load. Evidence is in the owned `.work/smoke-acceptance.log` and
`.work/rust-test.log`; no commercial game copy or commercial save was accessed.
The harness refuses modal requests instead of inventing an answer. Interactive
modal cancellation, commercial-game coverage and subjective audible quality
remain separate Manager acceptance checks.
