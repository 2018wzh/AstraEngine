# Artemis 上游差异

## 固定来源

- 完整 fork：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，主仓 submodule：`Emulator/ThirdParty/art3m1s-core`。
- 上游 HEAD（2026-10-17 核对，0.4.0）：`ad90a62cb192b119b7cf8589aa7f87a94bd747c8`。
- 唯一适配提交：[`ae967d8446fe99b97f6e1df93fb1b61d8ada2040`](https://github.com/2018wzh/art3m1s-core/commit/ae967d8446fe99b97f6e1df93fb1b61d8ada2040)，直接父提交为上述 HEAD；统一适配分支 `astra` 已推送 GitHub，远端 SHA 与主仓 gitlink 一致。
- 来源历史、Lua 5.1、NativeVulkan/NativeMetal、文字、FFmpeg 9 与许可证保留。

## 差异分类

| 类别 | 当前内容 | 处置 |
| --- | --- | --- |
| 输出钩子 | fallible `try_advance_frame` / `try_capture_frame_into`，推进与捕获分离 | 原生 owner 自主调度；统一 C ABI 交付三个有界槽的不可变 CPU lease |
| 生命周期 | 视频音频提取归播放实例所有；停止/替换/关闭取消并 join，清理临时 WAV | 保留必要边界，与通用 AVIO 修复分开 |
| 原生功能修复 | PF8 路径分隔符、AVIO 释放与所有权、onLoad 等待归属 | 独立 PR #5/#7/#9 均已复现失败前/通过后，等待上游合并 |
| 产品策略 | 显式 `NativeVulkanHardware` 选择 Discrete/Integrated/Virtual，拒绝 CPU/未知 adapter | 原 `NativeVulkan`/`PlatformDefault`、FFI 映射及默认设备排序保留 |
| 恢复通知 | 完整成功 load/reset 后生成一次性通知 | 原生 owner 清时钟余量、失效 Host 上下文并增加输出代次；旧 CPU lease 保持有效 |
| 构建闭合 | 独立 workspace；可选原作者桥使用精确 revision | 保留真实依赖，不改为相邻 Astra Family 的另一运行路径 |

可选桥 pin：`Alphaly2K/rfvp@ec204312e123b4839cec8e69e6237fb0374e5518`、`Alphaly2K/siglus_rs@28917920a3e5eeb9a5fa60eac31400422cd41fe2`。AstraEMU 不启用这两条桥；后者实际提供 `art3m1s-host-audio`。FFmpeg 版本声明已恢复 `9.0.0`，解析版本不变。

## 原生嵌入适配与上游修复

适配入口切换为 `astra_emu_query`；静态 `artemis_query` 返回同一 `CoreApi`，静态链接启用 `static-core`。删除旧 ProviderModule、Host advance 和必需 PCM sink。诊断先于 describe/probe/open；动态安装共享 diagnostic bridge，静态使用宿主 subscriber。不声明文字或资源 Hook。

会话专属 native owner 创建、驱动和销毁原 VM、Vulkan/Metal、FFmpeg、资源与存档。上游没有设备 mixer，沿用现有 SDK Symphonia/Kira 解码混音，改由该会话的 Kira CPAL backend 持有真实设备；设备失败直接返回错误，不选 NullAudio。取消独立设置原子状态、唤醒 owner 并失效阻塞的 Host window 服务；关闭等待 owner 和音频 worker，库继续驻留。未支持的原生 UI/media 命令仍明确报错。

Kira 保持 0.12.2，以发布来源 `e9cbb4608237bb339c3eaf498571000e6cce35ca` 为基线，使用独立通用修复 [2018wzh/kira@7d8961b20bce2ab8fd43b4bb34150d7e2898785c](https://github.com/2018wzh/kira/commit/7d8961b20bce2ab8fd43b4bb34150d7e2898785c)。原 CPAL stream-manager 丢弃 JoinHandle，仅 drop manager 不足以证明关闭。native 音频 owner 在成功、错误和 panic 退出均显式 `stop_and_join`，等待设备 callback/stream-manager 资源析构后才退出；普通 Kira Drop 仍非阻塞。Artemis 仅使用 StaticSoundData，不引入未使用的 StreamingSoundData scheduler 修改。workspace 与 SDK 必须统一此精确 Kira 来源，避免 StaticSoundData 跨 crate 实例不兼容。

音频取消使用独立结果：在源读取之间或 SDK 解码中取消时退出播放任务，再停止并 join 设备；不把正常关闭误报为 mixer 容量错误。只识别 SDK 的 `ASTRA_EMU_AUDIO_CANCELLED`，同时发生的真实读取/解码错误仍保留。音频命令发送区分成功入队、取消和队列故障；取消后 native owner 停止派发并走关闭路径，避免音频 worker 先退出时把正常关闭误报为 `ASTRA_EMU_ARTEMIS_AUDIO_QUEUE`。没有取消的队列断开及 join 后的真实 worker 错误仍返回失败。无窗口 Host 拒绝保留具体 `window.title` / `window.pointer` 操作，不跳过原生请求。

配置保留 `archive` 文件选择。CPU 捕获沿用原生完整 stage target，报告实际 L=R；窗口 drawable 变化不改原生尺寸。指针按正在显示帧的会话/输出代次校验，再执行一次 raster→logical 映射。读取成功的存档通知换代，旧输出不再接受输入，但旧租约可在 close 后任意线程释放。

原生修复分别提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)（onLoad，`ad20b8afd01ce2807510fa4e08629afc8e62345b`）、[PR #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)（PF8，`186ccc17fb10a6c6b3ee931d98a7c9d6830e722e`）及 [PR #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)（AVIO，`348b931068d31bd75eb5ec804967ce4b86448aee`）。最新远端核对均为 open、未合并。packed stereo 使用显式 `(i16, i16)`；同一非对称双声道样本在原 scalar 和 tuple 路径均正确，不将其报告为已证实缺陷。

## 结论与验证入口

既有基线曾使用原生媒体启用 API，自然播放至ケロQ《終ノ空remake -2025ver-》Windows 版的 1600×1200 标题；该结果不作为本轮 C ABI 验收。严格最小钩子内容仍待上游修复合并，基线证据见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，执行顺序见[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

2026-10-18 独立 Windows 验收已通过 build、Clippy（all-targets，`-D warnings`）、格式检查、13 项普通测试及以下 4 项硬件测试。硬件测试使用真实 GPU/音频设备与合成 PF6 作品，不代表商业视听验收；没有测性能或声明前台物理输入/听音通过。独立树只初始化 Artemis 与 CPAL，临时缩窄 workspace members 以避开未初始化的其他核心，保留全部依赖与 patch pin，验收后恢复 manifest/lock；Windows shaderc 使用映射短路径规避 MSVC 对象路径上限。

```sh
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-artemis
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-artemis --all-targets -- -D warnings
cargo fmt --manifest-path Emulator/Cargo.toml -p astra-emu-artemis -- --check
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-artemis -- --test-threads=1
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-artemis --test lifecycle -- --ignored --test-threads=1
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-artemis --lib native_device_completion_pause_cancel_reopen_and_decode_failure -- --ignored --test-threads=1
```

该组覆盖真实 native 创建/自主产帧、pause/resume、取消/关闭/同进程重开、跨线程归还 close 后租约、原生 save/load 代次失效及过小捕获缓冲区不破坏完整目标。恢复测试让 Host 失效回调等待关闭取消，已复现修复前的 `ASTRA_EMU_ARTEMIS_AUDIO_QUEUE`，修复后通过；音频测试覆盖播放完成、暂停恢复、取消、重开与坏音频明确失败。

商业 headless 验收使用新 ABI、独立游戏副本与复制到自有产物目录的 Manager：open 和首帧成功，保留两张 1600×1200 黑色启动帧后，`window.title` 实际返回 `ASTRA_EMU_ARTEMIS_PLATFORM_UNAVAILABLE: native window service is unavailable for window.title`，进程退出码 1。无窗口 Host 按契约拒绝此操作，没有伪造成功或跳过请求；该阻塞不是 PF8、AVIO 或 onLoad 的新上游缺陷证据。`reopen_count=2` 未进入第二轮。

随后重新复制隔离游戏，使用真实 Library API 注册，复制最新 Manager，在自有 PID/HWND 上通过 `PrintWindow` 与窗口消息操作实际 Slint 按钮。游戏库、启动按钮、运行诊断面板均可操作；日志记录真实 `session_started`，窗口保持黑色游戏区，`WM_CLOSE` 正常退出 0 并记录 `session_ended(reason=shutdown)`。该路径证明 Window Host 可达，不再以锁屏直接判定窗口路径不可用；本轮没有操作锁屏或伪造焦点。

当前阻塞是失焦暂停。独立核对的上游生产前端 [Art3m1s `2234686b9ccde9a4d6721ff0bfe8cf5364472bf7`](https://github.com/Alphaly2K/art3m1s/blob/2234686b9ccde9a4d6721ff0bfe8cf5364472bf7/lib/screens/player_screen.dart#L217-L245) 在 `inactive` 时设置 `_appActive=false`；[帧调度](https://github.com/Alphaly2K/art3m1s/blob/2234686b9ccde9a4d6721ff0bfe8cf5364472bf7/lib/screens/player_screen.dart#L603-L620) 要求 `_appActive`，否则停止 ticker。Flutter 的 [desktop inactive 定义](https://api.flutter.dev/flutter/dart-ui/AppLifecycleState.html) 是窗口可见但没有输入焦点。该前端声明 1.3.0/core 0.4.0 兼容，构建拉取 core master，没有精确 core gitlink；这里记录独立源码证据，不冒报同 pin 原生前端运行验证。没有发现后台推进配置，因此保留原生暂停策略，不为验收添加开关。真实窗口中脚本尚未推进到 `window.title`；媒体、正文、商业存读档、游戏重开和结局仍未通过，继续验收需要真实输入焦点。

商业作品入口为 `astra-emu-manager --headless <私有配置.json>`；`reopen_count` 保持 DLL 驻留，`frames` 是观察样本而非 VM tick。截图与 `.diagnostics` 留在 ignored 工作区，本次额外游戏副本及测试存档已删除，原游戏和原存档不改动。
