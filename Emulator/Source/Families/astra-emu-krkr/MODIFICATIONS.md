# Kirikiri (Kirikiroid2 lineage) Astra family fork record

`../../ThirdParty/kirikiri2/` is a source snapshot of upstream
[`fenghengzhi/kirikiroid2-web`](https://github.com/fenghengzhi/kirikiroid2-web)
at commit `7105b50822097d11b7b1f5894bf541509fd51350` (2026-09-07), itself a
maintained retarget of the Kirikiroid2 engine (TJS License) to CMake + vcpkg.
The snapshot keeps the engine (`cpp/`), `platforms/`, `cmake/`, the vcpkg
overlay (`vcpkg/`, `vcpkg.json`, `vcpkg-configuration.json`), and the top-level
`CMakeLists.txt`. Upstream-only development artifacts (analysis notes, task
plans, web UI assets, CI test scaffolding, docker and tooling) were not
vendored. The complete TJS License text stays in
`../../ThirdParty/kirikiri2/LICENSE`; `THIRD_PARTY_NOTICES.md` records the
attribution chain.

The local changes to the vendored tree are limited to the Astra hosted
environ, gated behind the `KRKR2_ASTRA_HOSTED` CMake option:

- `cpp/core/environ/astra/` is new: `astra_krkr_host.h` (the C ABI consumed
  by `astra-emu-krkr`) and `AstraHostedEnviron.cpp`, an offscreen
  `iWindowLayer` plus boot/tick/shutdown driving through
  `tTVPApplication::StartApplication` and `Application->Run()`. Input is
  injected with `TVPPostInputEvent`, composed frames are copied from the
  software renderer's primary texture in `UpdateDrawBuffer`, and an audio
  worker pulls mixed PCM and pushes it to the family bridge.
- `environ/CMakeLists.txt` excludes the cocos2d shell and the emulator UI
  forms and compiles the hosted environ instead; `cocos2dx` is no longer a
  link dependency for the hosted configuration. The platform executable
  chain, app resources, tests, and tools are skipped through guards in the
  top-level `CMakeLists.txt`, which adds the `astra-krkr-hosted` shared
  library target that embeds the plugin registration archives.
- `environ/win32/Platform.cpp` redirects engine state (`TVPGetDefaultFileDir`)
  to the family-provided save directory, converts `TVPExitApplication` into a
  flag (the engine must not exit the host process), suppresses blocking
  message boxes, and skips the command-line startup path. `TVPSetAstraHostedDirs`
  takes UTF-16 code units (`std::u16string`) on every platform so the C ABI
  stays identical.
- Linux hosted builds (`KRKR2_ASTRA_HOSTED` with `LINUX` on) reuse the
  Kirikiroid2 `environ/linux/Platform.cpp` line with the same hosted gates:
  the save-directory redirect, the non-exiting `TVPExitApplication`, and
  suppressed message boxes (the GTK dialog code and its includes are compiled
  out). The top-level `CMakeLists.txt` now defines the `LINUX` C++ macro that
  upstream received through the cocos2d package config, and the hosted target
  links `winmm` on Windows only. `cpp/core/CMakeLists.txt` expresses the
  forced astra builtin-compat include as `/FI` on MSVC and `-include`
  elsewhere. `visual/FontImpl.cpp` enumerates common Linux system font
  locations (Noto CJK, WenQuanYi, DejaVu) where the Windows branch reads
  `C:\Windows\Fonts`. `environ/astra/AstraHostedEnviron.cpp` replaces
  `MultiByteToWideChar` with a portable UTF-8 decoder and carries paths as
  `std::u16string`; `AstraHostedCompat.cpp` routes console output to stderr
  instead of `OutputDebugStringA`. `base/impl/StorageImpl.cpp` includes
  `windows.h` only on Windows. The build script selects the `astra-hosted`
  manifest feature explicitly, uses `x64-linux`/clang on Linux, and links the
  engine `.so` with an rpath instead of a Windows import lib.
- `sound/win32/WaveMixer.cpp` adds a hosted audio renderer branch and the
  `astra_krkr_hosted_fill_audio` pull entry; the mixer stays unchanged.
- `environ/Application.cpp`: `tTVPApplication::StartApplication` returns true
  after the startup script completes, so an embedded host can distinguish a
  successful boot; the platform `main()` entry points ignore the result.
- `environ/astra/AstraHostedEnviron.cpp` registers the `core`, `tjs2`, and
  `plugin` named spdlog loggers at boot; platform shells create them in
  `main()`, and unguarded call sites (the TJS exception path,
  `PlayerInternal.h`'s LOGGER macro) dereference them unconditionally.
- `cpp/external/minizip/unzip/ioapi.h` defines `ZOPENDISK64` as an alias of
  the plain opener: `zip.c` references the zlib-ng minizip entry while this
  ioapi has no disk-aware opener, and Kirikiri XP3 archives are never
  spanned. MSVC only warned about the implicit declaration; GCC 16 errors.
- `vcpkg/ports/libarchive` passes `-DENABLE_WERROR=OFF`: the tree's own
  `-Werror` turns the qualifier-dropping assignments that GCC 16 / Clang 22
  newly flag into build failures in the vcpkg Debug pass.
- `vcpkg.json` adds `libgdiplus` (non-Windows) to the `astra-hosted` feature:
  `cpp/plugins/layerex_draw` links it through the general (non-Windows)
  backend, and the engine-wide plugin link pulls it in on Linux.
- `cpp/plugins/layerex_draw/general/LayerExDraw.cpp` drops the cocos2d.h
  include under `KRKR2_ASTRA_HOSTED` (nothing in the file uses it; the cocos
  shell is compiled out).
- `cpp/core/visual/CMakeLists.txt` links `libjpeg-turbo::jpeg` or
  `jpeg-static` depending on which imported target the triplet exports.
- `CMakeLists.txt` (hosted target) routes shared-library output through
  `LIBRARY_OUTPUT_DIRECTORY` so the Linux `.so` lands in `lib/` next to the
  Windows import library.
- `visual/RenderManager.cpp` compiles the software render manager without the
  cocos texture adapter; `visual/FontImpl.cpp` skips the bundled-font load
  through cocos FileUtils (Windows system fonts are enumerated instead);
  `visual/impl/TVPScreen.cpp` reports a fixed offscreen size;
  `base/impl/StorageImpl.cpp` drops the cocos include.
- `cpp/core/{CMakeLists.txt,base,visual,movie}` gate the `cocos2dx` link and
  exclude the OpenGL render manager under the hosted option.

All changes are inert when `KRKR2_ASTRA_HOSTED` is off; the upstream platform
targets build exactly as before.

`astra-emu-krkr/src/` is the separate family adapter. It owns no engine
state: the engine is a process-global singleton behind the C ABI, mixed PCM
arrives from the engine audio thread through a global tap, and each
`advance` pumps one engine frame and pulls the composed RGBA frame into the
session snapshot. `open` settles startup ticks until the game creates its
primary window so the ABI's fixed `FrameInfo` is final. The engine runs on
wall-clock tick semantics (native Kirikiri behavior); `elapsed_ns` is
accepted as input but the family's tick policy is one engine pump per call.
