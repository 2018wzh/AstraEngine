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

上游窗口位只来自 `WINDOW_QUERY_STATE` 的真实结果：bit0 为全屏、bit1 为最小化。首次运行及一批 `WindowState` 通知处理完后查询，不逐 tick 查询，也不把 `focused` 或 `suspended` 转成这些位。Host 必须同时提供有效 `MODE` 和 `MINIMIZED`；无窗口、不支持查询或返回字段不完整时明确报 `window.query_state` 错误。查询在 snapshot 锁外进行，取消结果结束当前任务并正常关闭，不发布缺失状态。原有失焦、挂起和显式暂停策略保持不变。

配置保留 `archive` 文件选择。CPU 捕获沿用原生完整 stage target，报告实际 L=R；窗口 drawable 变化不改原生尺寸。指针按正在显示帧的会话/输出代次校验，再执行一次 raster→logical 映射。读取成功的存档通知换代，旧输出不再接受输入，但旧租约可在 close 后任意线程释放。

鼠标适配保留原生 `hide`（零显示、非零显式隐藏）与 `autohide`（毫秒、零停用）的独立含义，`null` 不修改已有设置。显式隐藏不会被移动解除；空闲隐藏在指针活动后解除。上游对应实现见 [Art3m1s `applyMouseConfig` / `notifyMouseActivity`](https://github.com/Alphaly2K/Art3m1s/blob/2234686b9ccde9a4d6721ff0bfe8cf5364472bf7/lib/engine/backends/art3m1s_engine_runtime.dart#L456-L519)，核心继续只发送原生命令，不增加绘制或窗口旁路。

Manager 的 `HostPointer` 统一管理原生光标可见性，Slint 游戏区域不再同时声明 `mouse-cursor: none`。Slint 1.17.1 更换控件光标形状时也调用 winit `set_cursor_visible`，Host 在事件分发后重新同步，不能用上次写入值缓存冒充当前 OS 状态。游戏区域以外、失焦、宿主模态/错误/工具面板显示时恢复系统指针并暂停空闲计时；重新进入后重新计时，仍遵守游戏显式隐藏。退出会话清除显式与空闲隐藏，重开不继承旧状态。窗口句柄和指针定位失败保留底层错误。纯逻辑回归覆盖这些状态转换和原生命令映射；Release 真实指针效果由用户人工验收，不自动启动游戏或注入输入。

原生修复分别提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)（onLoad，`ad20b8afd01ce2807510fa4e08629afc8e62345b`）、[PR #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)（PF8，`186ccc17fb10a6c6b3ee931d98a7c9d6830e722e`）及 [PR #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)（AVIO，`348b931068d31bd75eb5ec804967ce4b86448aee`）。最新远端核对均为 open、未合并。packed stereo 使用显式 `(i16, i16)`；同一非对称双声道样本在原 scalar 和 tuple 路径均正确，不将其报告为已证实缺陷。

## Release 构建与人工验收

当前交付只构建 Release 原生核心，由用户在真实 Manager 窗口中人工验收标题、正文持续输入、基本菜单、存读档和关闭重开；不要求结局或正式性能对照。不再保留隐藏 HWND、合成游戏硬件驱动、音频测试音、自动输入/重开/截图等验收代码。普通纯逻辑边界测试与生产取消、输出租约、真实窗口状态查询修复保留。

```sh
cargo build --manifest-path Emulator/Cargo.toml --release -p astra-emu-artemis
```

Windows 构建使用 MSVC、`VCPKG_ROOT` 指向 FFmpeg 7 安装、`LIBCLANG_PATH` 指向 libclang；Artemis 的 Lua 5.1 与 Manager 的 Luau 分开构建。产物为 `astra_emu_artemis.dll`，所需 FFmpeg DLL 随核心交付。独立树只初始化 Artemis 与 CPAL；为避免未初始化的其他核心参与 workspace 解析，可临时隔离 members，构建后恢复，不改变依赖版本或源 pin。Windows shaderc 使用映射短路径规避 MSVC 对象路径上限。

原生失焦暂停策略保持不变，不添加后台绕过。上游生产前端 [Art3m1s `2234686b9ccde9a4d6721ff0bfe8cf5364472bf7`](https://github.com/Alphaly2K/art3m1s/blob/2234686b9ccde9a4d6721ff0bfe8cf5364472bf7/lib/screens/player_screen.dart#L217-L245) 在 `inactive` 时设置 `_appActive=false`；[帧调度](https://github.com/Alphaly2K/art3m1s/blob/2234686b9ccde9a4d6721ff0bfe8cf5364472bf7/lib/screens/player_screen.dart#L603-L620) 随之停止 ticker。该前端声明 core 0.4.0 兼容，但没有精确 core gitlink；这里只记录源码依据。

已观察到真实窗口可以启动会话，并曾取得实际前台焦点；日志出现字体加载、Ogg 解码和私有自动存档。随后仅得到 `ASTRA_EMU_SESSION_FAILED` 与 `ASTRA_EMU_ARTEMIS_CLOSED`，没有具体底层错误，故保留为未归因失败，不认定可玩流程通过，也不归入旧 PF8/AVIO/onLoad 问题。停止自动化后不继续复现，不主动关闭用户窗口；仍由窗口引用的私有游戏和 runtime 保留，错误日志留在 ignored 工作区。

严格最小钩子仍待上游修复合并；来源记录见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)与[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

