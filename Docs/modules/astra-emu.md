# AstraEMU Module

AstraEMU 是旧 VN 模拟器和现代化套件。它复用 Astra Runtime、Media、Script、Plugin、Asset/VFS、Game Runtime Provider 和 Release Gate，但不进入 NativeVN 创作链路。

## Engine-native 架构

Manager 负责窗口、输入、配置、family 选择、provider selection、插件分发、报告、overlay、文本管线和滤镜 preset。Manager 自身是 Program target；被启动的 legacy case 通过 `AstraEmuRuntimeProvider` 作为 gameplay runtime session 运行。Provider 创建并驱动 AstraEngine `RuntimeWorld`，legacy family 以 engine-native in-process family plugin/provider 接入。

Manager 分为不依赖 UI 的 `astra-emu-manager-core`、只做 ViewModel/响应布局/accessibility/input routing 的 `astra-emu-manager-ui-slint`，以及装配平台服务的 `astra-emu-manager` Program。Slint host 持有单一窗口、event loop、surface 与同一套 wgpu 29.0.4 `Device`/`Queue`；游戏画面作为 GPU texture underlay 导入，Slint 只绘制 Manager 和 overlay，不做 CPU 整帧回读或跨设备复制。桌面采用导航、封面网格与 inspector 三栏；手机采用双列封面、bottom sheet 和底部导航；大屏移动设备切换桌面式自适应布局。

family 的影片命令只携带 playback id、VFS URI、模式和舞台尺寸。Manager host 在固定 tick 上推进时间线，用同一个 wgpu device 上传解码帧并合成到 underlay；modal 影片的 PCM 进入同一个 host audio mixer。`MediaFence` 必须同时匹配 token 与 playback id，完成、停止、save/restore rebind 不能依赖 wall clock。WMV/MPEG 走固定 revision 的增量 packet decoder；Windows MP4 为视频和音频分别持有 stateful WMF SourceReader，按 PTS 合并 packet，逐帧释放 BGRA、逐 chunk 追加 PCM，并在读取过程中执行 frame/byte/sample/timestamp budget。host 只预取 16 帧/500ms，PCM mixer 会裁掉已消费前缀。真实平台 E3 尚未形成，状态页继续保持 `IN_PROGRESS`。

Family plugin 注册 `LegacyRuntimeProvider` facade。Provider 通过 session 持有 archive resolver、旧 VM、media state、snapshot serializer 和 diagnostics，AstraEngine StateMachine 只调用 `AstraEmuRuntimeProvider` 暴露的 runtime step action。旧引擎语义必须落成 RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、StateMachineTrace、AwaitToken 和 save section；插件不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。

family ABI v3 另提供有界 `read_session_resource` 临时通道。音频和影片 effect 仍只携带脱敏 virtual URI，Manager/Headless host 在执行媒体命令时向当前 family session 请求 bytes，因此 `.bin` 等 archive 解析不会复制到 Manager。返回值只能在本次 host decode 中存活，不得进入 effect、RuntimeWorld、save/replay、日志、report 或 package；URI traversal、超限、session/context 漂移和 poisoned session 都必须阻断。

FVP runtime snapshot 使用有界压缩 envelope，并把 live texture 的精确 RGBA 状态写入 snapshot；path-backed texture 不能靠 restore 时重新读取 archive 来猜测已修改内容。host-command audio snapshot 只保存 slot/resource identity 和播放参数，restore 先清理 host-owned stream，再按 `LoadResource`、`Play` 顺序重建；commercial bytes 仍由当前 session 的有界资源通道提供，不进入 snapshot。host audio 会按声明、URI extension、受限 container magic 的顺序确定 codec；没有 active voice 时的 silent callback 不算 audible underflow，active voice 期间出现 underrun 仍会 fail-fast。

每个 `LegacyRenderFrameV1` 的 texture delta 与 draw list 必须按 tick 顺序消费。Manager 不得为了追赶 wall clock 丢弃中间 frame 后只渲染最后一帧，否则最后一帧可能引用尚未上传的 texture；队列溢出必须 blocking，而不是静默 pop。

`astra-emu-cli run` 是独立于 Manager 的 overlay-free 原生验收入口。它通过显式 `--engine`、授权游戏目录和可选 entry 创建 `AstraEmuRuntimeProvider` session，直接把 family frame 交给 Windows platform host，物理输入按 legacy 舞台宽高比路由。默认静音，只用于排除 Slint/overlay 后比较核心视觉行为；显式 `--enable-audio` 才启用平台音频。`astra-emu-cli headless` 使用同一个 provider lifecycle 和 `astra-platform-headless` 执行 `astra.user_input_sequence.v1`。Headless 不接受 `advance`、`choose` 等产品语义捷径，只消费序列化物理输入与固定时间控制，并输出脱敏 `astra.emu.headless_run_report.v1`、PNG/WAV 与 artifact manifest。save/restore 后的首 tick 必须显式使用 `RestoreContinuation`，host-owned audio/movie state 先重置再由 family effect 重建，后续 tick 才回到 `Live`。

Family 内部可以把 VM 映射为私有 scheduler、context、basic-block 和 action 状态机。多线程或多 context legacy VM 使用多个 child state machine，由 deterministic scheduler 按固定 `(priority, context_id, sequence)` 推进。公共 Runtime 只看到有序 effect、await、trace、snapshot hash 和 diagnostic。

`EMUCoreBridge` 只作为 extension point 保留，用于受限实验或外部工具桥接，不是 v1 主架构。

## Family 路线

| 顺序 | Family | 参考 |
| --- | --- | --- |
| 1 | FVP | 固定 rfvp `0.4.0` commit `657747252eb0d2c5fb4a340695ce6906c2d45133` 的 HCB VM、`.bin` VFS、media、syscall、snapshot/replay；v1 首发 family |
| 2 | Artemis | system script、tag executor、现代商业 VN case；后续 family |
| 3 | KrKr/KAG/TJS | 常见 XP3/KAG/TJS 生态 |
| 4 | BGI/Ethornell | BURIKO/DSC/BCS/BP 生态和公开参考实现 |
| 5 | SoftPAL | PAC/DAT、extcall 和传统脚本 VM 研究 |
| 6 | Siglus | Scene.pck、Gameexe、`.ss`、G00/media 研究 |
| 7 | Minori | PAZ + `.sc` 脚本研究 |

FVP 是 v1 首发 family；自动探测顺序仍按 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori 固定，用户 profile 的显式 family binding 始终优先。首版只装载官方随包 native provider：桌面和 Android 使用签名动态库，iOS 使用相同 registration contract 的静态 registry；不支持第三方安装、远程 catalog 或运行时下载 native code。

仓库保存同一 sanitized trace 在 derivative 与 pinned official revision 上的逐叶 differential golden，覆盖 40 个 opcode、HCB header、Variant 运算、stack/call frame、context wait 和 syscall 参数顺序。148 syscall catalog 另有逐项 neutral-input handler probe。两类测试都属于 E1/E2 边界验证，不能替代商业合法输入的完整 media/syscall parity 或 Windows/Android E3。Linux/macOS/iOS 通过独立 CI target 验证 package/provider/host compile；Android package workflow 构建 arm64-v8a 与 x86_64，iOS 走静态 archive registry，但在真实 CI artifact、签名或设备报告存在前，状态保持开放。

每个 family 的实现级调研、格式说明、脚本演出拆解和工具命令放在 [../emu/README.md](../emu/README.md)。研究页可以保留旧引擎原始术语；产品 contract 以本页和 [AstraEMU Legacy Runtime Provider Contract](../contracts/astraemu-ipc.md) 为准。

## Luau Patch / Decode

EMU 用户脚本统一使用 Luau。Trusted Project Profile 可以开启 read-only VFS mount、patch overlay、decode transform、text/media hook、VM trace、diagnostic 和 deterministic effect intent。状态注入只能变成 `LegacyEffect`、Blackboard、input 或 tag intent，在 fixed tick 边界进入 Runtime。脚本请求未授权 key 提取、商业保护处理、访问控制规避、raw filesystem/network/system call 或 native handle 时，Manager 隔离禁用该脚本并生成稳定诊断。只有 case profile 明确允许无补丁模式时才能继续，否则阻断启动。

## Text / Translation / Filter

`TextCaptureEvent` 进入 Manager 的 `TextCapturePipeline`。默认 report 只写 hash、长度、source ref 和 speaker metadata；用户按游戏 opt-in 后才能把原文与译文写入本地 SQLite。首发 `OpenAICompatibleTranslationProvider` 绑定 ECNU Open API：profile 显式声明 endpoint、Responses 或 Chat Completions、model、目标语言、上下文、16 KiB 预算和 secret reference，不自动切换协议、endpoint 或 model。默认最近 10 句、可配置 0–32 句，超限在句边界确定性截断。全局授权不自动失效，overlay 始终显示 endpoint、model 和发送范围；timeout、限流或协议错误保留原文并熔断，只有用户操作可恢复。凭据只从平台 secret store 解析，不进入 SQLite、日志、报告、save/replay 或 package。

滤镜复用 Media `FilterGraph`。AstraEMU profile 可以绑定 final-frame preset 和 per-layer preset；per-layer 只依赖 `PresentationCommand` 的 layer id 或 role。family 不提供 layer metadata 时，只启用 final-frame 并输出 diagnostic。不新增 family 专属 shader/filter API。

## 验收

每个 family 必须产出 local case report，并通过 full-flow `astra.user_input_sequence.v1` JSONL：boot、main route、choice、text、voice、BGM、SE、movie、system menu、config、save/load、backlog、replay 和 shutdown。报告只包含 hash、offset、entry count、coverage、diagnostics 和脱敏 metadata，不能提交完整商业 payload、图片、音频、视频、完整剧情脚本、私有绝对路径或保护绕过材料。

Legacy Runtime Framework 的 session、step、effect 和 snapshot 设计见 [AstraEMU Legacy Runtime Framework](../implementation/astraemu-legacy-runtime-framework.md)。VM 到状态机的映射见 [EmulatorCore StateMachine Mapping](../implementation/emulator-core-state-machine.md)。FVP 首发 family 的格式、脚本、presentation/media 与验收清单见 [FVP Research Index](../emu/fvp/README.md)；Artemis blueprint 只表示后续 family 研究，不再阻塞 v1。
