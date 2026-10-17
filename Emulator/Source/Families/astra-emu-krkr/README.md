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
  `32130382185b60fe047ae9d98f8f3e4795c4db1e`.
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

Host request failures retain their original typed status, code and message for
the current native operation. If TJS catches a window failure and then requests
an error-reporting modal, a second Host refusal cannot replace the first error.
An error handled by the script is cleared before the next native operation.
Modal caption/message diagnostics retain the script's original error context;
the Host still refuses unavailable window/modal operations without answering them.

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

## Initial native acceptance

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
- Cross-game `System.exePath` isolation: the cancellation fixture asserts its
  own `cancel-loop/` application root before entering the infinite VM loop.

Three successful native opens and one cancelled open completed in that one
process/module load. Evidence is in the owned `.work/smoke-acceptance.log` and
`.work/rust-test.log`; no commercial game copy or commercial save was accessed.
The harness refuses modal requests instead of inventing an answer. Interactive
modal cancellation, commercial-game coverage and subjective audible quality
remain separate Manager acceptance checks.

## Manager 与商业启动验收（2026-10-18）

本轮独占树为 `AstraEngine-emu-krkr-commercial-20261018`。源码 pin 和
layerExImage 未变；C++ 适配仍在主仓 `native/`，未增加 fork 适配提交。
自有 Release CMake target 重建通过。WinFlexBison 并发生成曾报
`b4_symbol_if`，先用 `--target GENERATE_CPP_FILES --parallel 1` 生成解析器，
再用 `--target astra_krkr_native astra_krkr_lifecycle_smoke --parallel 6` 完成构建。

- `astra_krkr_lifecycle_smoke <DLL> <fixture-copy> --public-fixture` 返回 0。
  新增的两项首错用例通过：Host typed title 拒绝后再拒绝错误模态，原始
  status/code/message 不变；没有 window callback 时仍返回
  `KRKR_WINDOW_SERVICE_REQUIRED`。原有 GPU、PNG、字形、OpenAL、静态帧、
  输入变化、旧 lease、同 DLL 重开、无限 TJS 取消和 `System.exit` 用例通过。
- 实际 Manager 使用 `--headless .work/positive.json` 返回 0，同进程运行两轮，
  两轮均有真实按键画面变化、`KRKR_FINISHED exit_code=0` 和成功 close。
  公开输入由原 fixture 复制，只去掉 `win.caption` 请求；没有修改商业脚本。
  这证明公开 fixture 的 Manager 正路径，不代表商业游戏结局。
- 四张 Manager PNG 均为 320×240。初始底色、矩形和透明 PNG 合成像素分别为
  `(32,64,96,255)`、`(224,32,64,255)`、`(16,160,48,255)`，字形像素 784。
  两轮初始截图按 native stride 补零后 hash 均为 `2466111679093113121`，
  与 native smoke 的完整帧一致；按键后左上像素变为 `(33,49,65,255)`。
  `ALPHA_IGNORED` 保留原 RGBA 字节和原窗口禁用 blend 的显示语义，没有逐像素改写。
- `--headless .work/title.json` 返回 1，首错为
  `KRKR_NATIVE: KRKR_WINDOW_SERVICE_REQUIRED`；日志保留 `startup.tjs(4)`，
  后续错误模态不再把它覆盖成 `ASTRA_KRKR_HOST_MODAL_FAILED`。
- SenrenBanka 隔离完整副本实际读取 `data.xp3`（1278 文件），随后原
  `startup.tjs(1)` bytecode、VM ip 7 请求 `System.inform`：
  caption `Information`，message `startup failed: corrupted execution file`。
  Headless Host 拒绝该真实模态，最终返回 `KRKR_MODAL_SERVICE_REQUIRED`，进程返回 1。
  未到达游戏画布、代表剧情或结局。没有答复模态、修改脚本或引入旧 DLL/TPM
  兼容层；桌面锁屏下未做交互窗口验收。未用相同 pin 的独立原生入口复现，
  因此不把该商业启动阻塞归因于上游。

运行前已确认 `ApplicationSpecialPath.h:56-61` 将存档放在游戏副本
`savedata/`，`IndividualConfigManager.cpp` 将偏好放在副本目录。
原游戏和旧存档未写入；本轮额外商业/公开游戏副本及其存档已删除。
ignored `.work/` 保留 `smoke-final.log`、三个 `*-manager-final.log`、
失败前日志、`pixel-evidence.json`、`acceptance-results.json`、截图和命令记录。
未采集性能或主观听音结论。
