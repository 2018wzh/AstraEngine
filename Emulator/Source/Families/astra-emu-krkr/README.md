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
  `5b2eccd0a51b6b47bb845f48acfd12bd2167c477`.
  The publication branch is `astra`; temporary integration branches are retired.
  The retained Windows patch is separate commit
  `cae1bd8b98a817505f5a973ac00e6b925fbe35e3`; lifecycle fixes follow it.
- `layerExImage`: `23a9214f9b6e9bc3267b964707efa269c041b919`, compiled unchanged.
- The retained tracked Windows/native patch and all five untracked source/overlay
  files were copied from the read-only prior checkout before lifecycle changes.
  The eleven-file retained native seam is the origin of `native/`; `HostAudio.h`
  and the private process ABI exports were removed by this cutover.
- `native/include/astra_emu.h` is the generated shared header at AstraEngine
  `6799938dc33b25bcca4d9a11ddbea1366bf7d93c` (ABI v3). Regenerate/copy it with the shared
  crate whenever that ABI revision changes; do not edit the header independently.
- The Astra-specific C++ adapter lives in this main-repository `native/`
  directory. The fork supplies retained build/platform seams and general
  lifecycle/path-cache fixes; it does not contain a separate Astra adapter
  commit. This provenance is unchanged by the commercial acceptance work.

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
The final game canvas declares ignored alpha: Manager presents its original RGB
as a complete image, rather than compositing it as a straight-alpha layer.
This matches the pinned core's `TVPWindowLayer::UpdateDrawBuffer` in
`cpp/core/environ/cocos2d/MainScene.cpp`: it sends the same texture to
`frame_ready` (lines 1112–1116) and presents it with
`DrawSprite->setBlendFunc(BlendFunc::DISABLE)` (line 1221).

`startup.tjs` directories and conventional `data.xp3` directories use the
original selector entry rule. Only Cocos UI/cache files use a session-owned
writable directory; cleanup removes only that directory. Original
`ApplicationSpecialPath` game `savedata` and `Kirikiroid2Preference.xml` locations
are unchanged. Never run acceptance against the sole commercial save copy.
Only compiled original plugins are registered; arbitrary legacy TPM/DLL loading
is not introduced. Unsupported platform requests fail instead of inventing a
modal answer. No text-hook capability is advertised.
The descriptor names the emulator `KiriKiri2` and the engine `KiriKiri`. It
declares no configuration and no render-extent capability; open rejects a
nonzero `render_extent` with `KRKR_RENDER_EXTENT_UNSUPPORTED`.

Host request failures retain their original typed status, code and message for
the current native operation. If TJS catches a window failure and then requests
an error-reporting modal, a second Host refusal cannot replace the first error.
An error handled by the script is cleared before the next native operation.
Modal caption/message diagnostics retain the script's original error context;
the Host still refuses unavailable window/modal operations without answering them.

## Build prerequisites

Windows x64, MSVC 14.51 (or a compatible verified toolset), CMake >=3.28, Ninja,
WinFlexBison 2.5.25/Bison 3.8.2, native OpenGL and an OpenAL playback device.
Use the retained original vcpkg manifest/overlays: FFmpeg 3.3.9 with Yasm,
Cocos 3.17.2, SevenZip 26.04, UnRAR and all original plugins. Do not substitute
SDK FFmpeg, skip layerExImage, or use the Artemis renderer.

FFmpeg is dynamically linked (LGPL-2.1): the `x64-windows` triplet builds
`avcodec-57.dll`, `avformat-57.dll`, `avutil-55.dll` and the other FFmpeg DLLs,
and the core imports them. libbpg selects the HEVC decoder through the public
`avcodec_find_decoder_by_name("hevc")`, and `NativeSession` calls
`TVPInitLibAVCodec()` during single-threaded startup so the decoders are
registered before any BPG image loads. Ship every FFmpeg DLL that
`dumpbin /dependents astra_emu_krkr.dll` lists (from
`.work/krkr-native/vcpkg_installed/x64-windows/bin/`) in the core directory.

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
  "-DVCPKG_INSTALL_OPTIONS=--x-buildtrees-root=$root/.work/krkr-buildtrees;--x-packages-root=$root/.work/krkr-packages;--downloads-root=$root/.work/krkr-downloads"
cmake --build .work/krkr-msvc --target astra_krkr_native
```

The native target copies app-local dependencies and `Resources/` into
`.work/krkr-msvc/embedded-adapter/`. For linked Manager builds set
`ASTRA_KRKR_NATIVE_LIB_DIR` to the directory containing `astra_emu_krkr.lib`;
no child executable is packaged. The native DLL itself is the dynamic core module.

Stage it as a core directory (`<output>/astra_emu_krkr/` with `core.json`). Pass
every app-local DLL of the output directory plus the closure that
`check_core_deps.py` finds in the vcpkg `bin/` (the FFmpeg 3.3.9 DLLs and their
`iconv-2`, `liblzma`, `bz2`, `zlib1` dependencies) and the MSVC OpenMP redist
directory (`vcomp140.dll`). `msvcp140.dll`, `vcruntime140.dll` and
`vcruntime140_1.dll` ship at the package root, not in the core directory:

```powershell
$adapter = ".work/krkr-msvc/embedded-adapter"
$openmp = (Get-ChildItem "$env:VCToolsRedistDir/x64" -Directory -Filter 'Microsoft.VC*.OpenMP' | Select-Object -First 1).FullName
python Tools/AstraEMU/check_core_deps.py closure --library "$adapter/astra_emu_krkr.dll" `
  --search-dir $adapter --search-dir .work/krkr-native/vcpkg_installed/x64-windows/bin --search-dir $openmp
python Tools/AstraEMU/stage_core.py --library "$adapter/astra_emu_krkr.dll" `
  --plugin-id astra-emu-krkr --version 0.1.0 --resources "$adapter/Resources" `
  --dependency <each DLL listed above> `
  --license kirikiri2/LICENSE=Emulator/ThirdParty/kirikiri2/LICENSE --output "$stageRoot"
python Tools/AstraEMU/check_core_deps.py check --core-dir "$stageRoot/astra_emu_krkr"
```

`$stageRoot` must be a new directory. The release workflow
(`.github/workflows/emu-release.yml`) runs the same staging and check.

## Manual Release acceptance

Acceptance runs only in the real Release Manager. The user selects the game,
operates native menus and save/load, then closes and reopens it. Scripted input,
capture/reopen drivers and the native lifecycle harness have been removed.
Build success is not a manual-playability result.

The production adapter preserves the first Host service error, the original
OpenGL/OpenAL pipeline, and `ALPHA_IGNORED` for the final native canvas.
WindowEx now initializes its optional overlay owner before destruction; the
previous uninitialized pointer could cause an invalid delete during shutdown.
This generic lifetime fix does not alter game scripts or use process isolation.

Earlier development observations and raw diagnostics remain in Git history and
ignored evidence. They are not re-run automatically. Original game files and
existing saves must remain unchanged; user-reported manual interruptions are not
engine failures or completed routes.
