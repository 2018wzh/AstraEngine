# KrKr Hosted 引擎集成

本页记录 KrKr family 在独立 Host 架构下的真实接入方式：以 vendored Kirikiri 引擎核心为进程内 engine，通过 Family ABI 暴露给 Manager。实现位于 `Emulator/Source/Families/astra-emu-krkr/`（family adapter）与 `Emulator/ThirdParty/kirikiri2/`（vendored 引擎）。

## 选型

上游基线固定为 [`fenghengzhi/kirikiroid2-web`](https://github.com/fenghengzhi/kirikiroid2-web) revision `7105b50822097d11b7b1f5894bf541509fd51350`（2026-09-07，TJS License）。选择理由：

- Kirikiroid2 血统的引擎核心是唯一被证实能在非 Win32 平台运行商业 Kirikiri 游戏的实现路线；该 fork 持续维护（2026-09 活跃），已把 zeas2/Kirikiroid2 的核心重定向到 CMake + vcpkg 工程化构建，并新增 `KAGParser`、`psbfile`、`psdfile`、`motionplayer`、`textrender`、`json`、`PackinOne`、`extrans` 等常见 native 插件的 C++ 内置仿真。这些仿真正是 win32 `.dll` 插件（无法被 x64 host 加载）的替代路径。
- 引擎自持软件渲染管理器（`iTVPRenderManager` 软件实现）与自绘音频混音器（`WaveMixer`），平台壳（cocos2d environ）是薄胶水层，可整体替换；krkrsdl2 等替代方案加载真实 `.tpI` 插件时要求进程位数与插件一致（本作全部为 x86），与 x64 host 进程内 ABI 冲突。
- 排除项：krkrz 上游（Win32-only、停更）、Kirakira（AGPL 且无插件能力）、TsangAsuna/krkrz（无验证）。

## 结构

```
Emulator/ThirdParty/kirikiri2/          vendored 引擎（TJS License，见 MODIFICATIONS.md）
  cpp/core/environ/astra/               Astra hosted environ（本仓新增）
    astra_krkr_host.h                   C ABI（唯一跨语言边界）
    AstraHostedEnviron.cpp              离屏窗口层 + boot/tick/shutdown 驱动
  cpp/core/{sound,visual,base,environ}  若干 KRKR2_ASTRA_HOSTED gated 补丁
Emulator/Source/Families/astra-emu-krkr/  Family ABI adapter（独立 Cargo workspace，cdylib）
```

`KRKR2_ASTRA_HOSTED` CMake option 关闭时 vendored 树与上游行为一致；打开时替换平台壳：

- environ 编译 `environ/astra/AstraHostedEnviron.cpp`，剔除 cocos2d 壳与模拟器 UI forms；cocos2dx 不再进入链接。
- 引擎状态目录（配置、存档、dump）重定向到 family 提供的 save 目录；`TVPExitApplication` 变为置位终止标志（不允许引擎 `exit()` 杀掉 host 进程）；阻塞式 MessageBox 抑制为日志。
- 音频：`WaveMixer` 新增 hosted 渲染分支，family 侧 pacing worker 按 48 kHz/S16/立体声拉取混音 PCM 并写入 host AudioSink。
- 渲染：主窗口 `iWindowLayer::UpdateDrawBuffer` 把合成好的 RGBA 帧复制进引擎侧帧缓冲；`astra_krkr_copy_frame` 供 Rust 侧按尺寸查询+拷贝。
- vcpkg manifest 增加 `astra-hosted` feature（引擎依赖，无 cocos2dx/glfw/bullet）；hosted 配置以 `VCPKG_MANIFEST_NO_DEFAULT_FEATURES=ON` + `VCPKG_MANIFEST_FEATURES=astra-hosted` 选择。

## Family ABI 映射

| Family ABI | 引擎语义 |
| --- | --- |
| `probe` | 目录含 `.xp3` 即报告 `krkr.xp3`（置信度 90 permille，允许其他 family 竞争） |
| `open` | 进程互斥（单 session）；boot 后持续 settle tick 直到首帧合成，`OpenResponse.frame` 即最终尺寸 |
| `advance(elapsed, events)` | 输入经 `TVPPostInputEvent` 注入（VK 码映射、Y 翻转由窗口层处理），随后一次 `Application->Run()` 泵帧，再拉取帧快照 |
| `frame` | 引擎侧帧缓冲快照（RGBA8 opaque），一次拷贝 |
| audio | 引擎音频线程经全局 tap 推 PCM；host sink 取消即熔断 |
| `close` | `TVPTerminateAsync` + 一轮 `Run()` 完成 uninit，join 音频 worker，释放进程互斥 |

引擎为进程级单例（原生全局状态），ABI 的一个进程一个活动 session 约束在此是硬性的：第二个 `open` 直接返回 `ASTRA_EMU_KRKR_ENGINE_BUSY`。

## 构建与测试

family crate 是独立 Cargo workspace（不进主 workspace），`default` feature 触发 build.rs 经 CMake 构建引擎静态库并链接为 cdylib 动态插件：

```bash
cd Emulator/Source/Families/astra-emu-krkr
cargo build --release          # 需要 VCPKG_ROOT、CMake、MSVC x64 环境
cargo test                     # 纯 Rust 单元路径
ASTRA_KRKR_TEST_GAME=/path/to/game cargo test --release -- --nocapture
```

headless 冒烟测试（`tests/headless_game.rs`）驱动静态 provider：boot、120 帧 settle、Enter 注入、帧完整性、PCM 计数、close，全部走真实引擎进程。

## 状态

- vendored 核心 + hosted environ + family adapter 代码已就位（E0/E1）。
- Windows hosted 构建与真游戏 headless 验证进行中；结果与失败路径如实记录于本页与 `game-observations.md`，完成前不标记 DONE。
- 翻译文本替换：engine 文本流尚未经 `TextCaptureEvent` 桥接，family 不声明 TextReplacement capability。
