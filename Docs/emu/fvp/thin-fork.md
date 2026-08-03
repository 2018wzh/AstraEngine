# RFVP Thin Fork 与 Hosted Adapter

## 目的与当前状态

FVP 采用 `2018wzh/rfvp` 的 `astra-hosted` 分支作为小型、可重放的 fork。补丁基底固定为 RFVP `0.5.0`（`3b5ea6c96a925c12f95aef8554905e8fecbc77c3`）；为复用已验证的文本 surface 所有权实现，补丁栈还保留一个经审查、未改写的上游移植补丁 `a94fa18`。除此以外只补充 host-neutral `hosted-core`，不把 Astra 类型、RuntimeWorld、序列化格式、错误码、路径约定或平台 GPU/audio handle 写入 RFVP。

截至本文更新，fork 已固定 upstream base，并已加入有界 `HostedSession`、`HostedStepInput`、`HostedStepDelta`、session-owned globals/text、snapshot/restore、canonical state identity、最多 64 MiB 的 opaque snapshot bytes、Shipping/Evidence 固定 trace ring、结构化 `HostedLogRecord`/phase observer、`.bin` metadata 后的按需 range-read，以及仅含 URI/长度的视频资源 delta。Astra 的注册 case image 和动态 host VFS 都经无平台 handle 的 hosted VFS/clock port 打开；动态 provider 每 tick 只接收一个 hosted delta，转换为 `ScenePacket`、媒体命令、local-only text lease 和 `PreparedCommit`。named audio 在 fork 中只排入 source URI，资源字节由 adapter 后续经 session-bound host VFS 读取；RFVP core 不再通过内部或进程 VFS 读取这类资源。`HostedLogRecord` 只含稳定 code、level、phase 和计数；adapter 映射为 `astra-observability` event，绝不跨边界转发 RFVP message。`ScenePacket` translator 将 create/partial-update/destroy 转为有界资源操作，先验证完整事务再替换元数据；restore 后的纹理重建显式开始新的资源 epoch，Manager/WGPU 与 CLI CPU reference 都先清除旧资源、独立复验 commit，再写入各自资源存储；solid draw 使用保留的白色 sentinel texture，不借助平台 handle。`astra-emu-fvp` 已不再编译依赖本地 RFVP vendor core。旧 render-frame、逐 syscall journal 和逐 opcode 字符串 trace 不再参与 v5 provider。

本地公开 Win95 Painter sample 的 signed dynamic FVP v5 已通过 120 fixed step 的 Headless run：6 条物理输入均被 host 消费、4 个实际 CPU frame、一个 PNG checkpoint、VFS 2 资源/20 次 range-read、snapshot round-trip 和正常 host shutdown 均通过；人工查看 checkpoint，窗口、工具栏、调色板、画布与底部状态栏可见且无残缺。该输入序列未产生可见笔划，因此它只证明 input transport，不证明脚本交互语义。CPU reference 该次 step p95 为 11.22 ms，4 次 raster 的中位数为 248.12 ms；它用于确认 scene dedup 没有退化为逐 tick 全帧光栅化，不是 GPU 或 RFVP 对比结论。该 sample 无音频、未到脚本 terminal，也没有媒体、路线、性能 soak 或 Windows E3，所以只构成 hosted-v5 的局部 Headless E2/视觉证据，不能作为完成声明。

另一个由公开无资源脚本生成的 signed dynamic FVP v5 Headless lifecycle case 已在 2 step 达到 `terminal`，并同时通过 snapshot round-trip、PNG checkpoint、正常 session shutdown 与 host shutdown。该 checkpoint 是预期的空黑 frame，只证明 `ExitMode(3)` 到 hosted terminal 的生命周期传播，不是产品视觉、路线或媒体证据。

公开生成的 input case 将主按钮 edge 送入 hosted session，并在下一固定 step 将一个已分组的 64×64 tile 从黑色改为红色；其 3 step signed dynamic run 同时通过 snapshot round-trip、PNG checkpoint、terminal 与 shutdown。人工检查 checkpoint，左上角 tile 为红色且其余画面保持黑色。这是 physical input → VM → `ScenePacket` → CPU reference 的可见 E2，证明输入 transport 和语义提交；它不替代实际游戏路线、文本、媒体或平台 E3。

公开生成的 audio case 以 `AudioLoad`/`AudioPlay` 请求一个 Ogg source URI，并在四个 `ThreadNext` 后停止；其 62 step signed dynamic run 通过 snapshot round-trip、checkpoint、shutdown 与 VFS resource policy，输出 49,600 个音频 frame 和两个真实 WAV artifact，audio meter hash 非空。它覆盖 RFVP named-audio → URI-only hosted delta → adapter session resource read → Headless decoder/output 的链路；这是局部媒体 E2，不等同于真实游戏音频、视频、PTS/route 或 Windows E3。

2026-08-02 的授权本机安装 smoke 以签名动态 v5 family 完成 300 fixed step：170 个 scene commit/raster frame、两个 PNG checkpoint、同 session snapshot round-trip 和正常 shutdown 均通过，VFS 记录 14 个资源、55,011 次受限读取、41,648,158 bytes，且没有 blocking diagnostic。人工查看后一个 checkpoint，标题画面完整可见；前一个启动期 checkpoint 为空白，未被当作视觉通过项。该 run 没有路线输入、terminal、音频或视频证据，CPU reference 的 step p95 为 58.08 ms，只是 Evidence profile 下的本机趋势，不能视为性能放行、媒体 parity 或 Windows E3。

同一 build/profile 的 900 fixed step idle 延伸运行也通过：342 个 scene/raster frame、两个较晚 checkpoint、snapshot round-trip、正常 shutdown 和同一受限 VFS 账本均无 diagnostic。人工查看两个后期 checkpoint，标题淡入和静止阶段均完整，确认前述资源重发布在更长的 idle 段没有丢失纹理。该延伸仍没有路线输入、terminal、音频、视频或 soak 承诺，CPU reference 的 step p95 为 53.18 ms，只保留为本机 Evidence 趋势。

在 `0.5.0` 基底与当前 fork pin 上，复用了既有本机排障中验证过的 11 条物理输入序列：恢复/focus、650 tick 前置、Enter down/up，以及 1,256 与 1,260 tick checkpoint。签名动态 v5 Headless 运行完整消费该序列，在 1,261 fixed step、720 scene/raster frame、488,800 audio frame 后通过 snapshot round-trip、正常 shutdown、受限 VFS 和非静音非削波 WAV artifact，且没有 diagnostic。两个转场 checkpoint 的图像 hash 相同，人工检查均为完整标题画面；因此该复跑只证明基底迁移后历史输入格式、键盘 edge 和当前 host 生命周期兼容，**不**把它计为菜单选择、路线推进或 RFVP parity 证据。后续真实路线验收必须先记录当前 build/profile 下经 host 消费且产生状态/画面变化的输入意图，再以独立 checkpoint 验证。

该复跑暴露了 hosted-core 漏掉 `RfvpEvent::KeyDown`/`KeyUp` 到 `InputManager` 的映射：CLI 虽记录 `confirm` edge，fork 却只转发 pointer/wheel，因而键盘输入不能到达 VM。fork 已在 `90af8f88cb10ccad70dfc74fa914c286993aaf3d` 修复完整 FVP key-bit 映射并由 Astra 精确 pin。使用同一受控安装重新运行后，4,200 fixed step 的显式确认序列产生 814 scene/raster frame、2,840,000 audio frame、非静音非削波 WAV、完整标题菜单 checkpoint、snapshot round-trip 和零 diagnostic；随后以 pointer click 选择首个菜单项的 5,100 step run 产生 942 scene/raster frame、3,637,156 audio frame及多段非静音非削波 WAV，两个点击后 checkpoint 为完整黑场。后者已证明键盘/鼠标 edge 参与真实脚本、菜单状态和媒体链路，但黑场尚未区分为内容转场、等待还是视频阶段，不能作为正文视觉、route terminal 或视频 parity 结论。

## 不可跨越的边界

```text
RFVP hosted-core
  HCB VM / FVP semantics / hosted ports / typed delta
        │ single bounded delta
        ▼
Astra FVP adapter
  ABI validation / ScenePacket / media request / PreparedCommit
        │ validated family effects
        ▼
RuntimeWorld + platform renderer/audio/media
```

- RFVP 保持 upstream 的 app、window、GPU、software renderer 和 UEFI 文件布局；这些原生 host 不参与 Astra hosted build。
- Astra adapter 是唯一可见 `LegacyRuntimeProvider`、动态 ABI、VFS policy、资源授权和 `RuntimeWorld` 接口的一层。
- EngineCore 不依赖 RFVP、legacy VM、native decoder 或任何 FVP DTO。
- 插件不能打开宿主目录、保存宿主路径或持有 platform handle。资源身份、revision、范围和预算由 Astra host 验证。

## 更新与 rebase

1. 在 fork 分支上逐个提交可审查 patch，标题使用 `[hosted]`。补丁顺序以 `0.5.0` 为基底；任何获准复用的上游成熟移植先单独记录来源、范围和理由，再置于 hosted patch 之前。
2. 每个 patch 必须只修改相邻 RFVP 模块，并在提交前记录 upstream base、patch id、许可证来源和验证命令。
3. Astra 的 Cargo dependency 只钉住已推送的 git revision；禁止再次 vendor RFVP 或使用浮动 branch/tag。
4. 更新 upstream 时先在 fork rebase，运行 upstream 回归和 hosted-core 测试，再更新 Astra pin。不得把 Astra adapter 修改混入 fork rebase。
5. dynamic FVP descriptor、CLI 和 Manager 的 feature fingerprint 都从 `astra-emu-fvp` manifest 的 `hosted_fork_revision` 读取；pin 更新若未同步进入这三个 identity，构建必须失败，不能继续签发把旧 revision 写入证据的 binary。

## 性能与正确性原则

- Shipping 只传 scene/resource/media 的语义 delta；不得逐 opcode 分配 trace、格式化 opcode 字符串、序列化完整状态或复制完整 RGBA framebuffer。接收端以尺寸、draw list 和已验证资源内容 hash 计算轻量 visual identity，再校验并提交 `PreparedCommit`；不得为了帧去重再次序列化包含纹理像素的 commit。
- Evidence 使用固定容量 crash trace ring 和显式 profile。它是受限诊断，不得改变 Shipping 执行、状态 hash 或资源访问。
- 纹理按 id/generation 管理：创建、局部更新、销毁均为显式操作；adapter 在资源/profile/binding 检查完成前不得提交部分帧。
- `.bin` 只读取受限 metadata；entry 通过受限 range-read 提供。禁止启动时预载整包，也禁止把商业 bytes 写入 save、replay、日志或报告。
- named hosted audio 保留 source URI 并转换为受 policy 约束的资源命令；没有 source identity 的 encoded bytes 不能伪装成 URI 或跨 ABI 传递，必须以 blocking diagnostic 停止提交。PCM stream command 仍可在既有限额内转换。
- `PreparedCommit` 在 host 完成 ABI、资源、预算、hash、profile 与 binding 验证后才可提交。任何缺失或不匹配都必须阻断。
- `astra-emu-cli headless` 的性能证据必须同时指定 `--performance-budget`、`--performance-report`、`--perfetto-trace` 和 `--performance-trace-manifest`。该模式固定 1,200 个 warmup presentation 加 72,000 个测量 presentation，拒绝 Debug 或 dirty build、CPU renderer、非 DX12 timestamp-query GPU、`frame_sample_interval != 1`、resume/export snapshot 和不完整输出集。它把 shared `astra.performance_report.v1` 与 `astra.performance_trace_manifest.v1` 的 hash 回写到 Headless v3 report；report/manifest 只保存身份、计数和 hash，不能携带商业 payload 或本地路径。

## 原版与 hosted 链路对照

对照基准固定为 RFVP `0.5.0`（`3b5ea6c96a925c12f95aef8554905e8fecbc77c3`）和 Astra 当前 pin `eff7c42f63c3476b1a331a99dc2e72fbcb6d0df0`。原版在同一进程内从 `GraphBuff generation` 进入 `GpuPrimRenderer`：generation 未变时直接命中 cache；同尺寸 `RawRgba` 更新调用 `GpuTexture::update_rgba8`，最终只对已有纹理执行 `queue.write_texture`。资源未变化时不会重建 GPU texture，也不需要跨 renderer、ABI 或 `RuntimeWorld` 复制像素。

hosted 路径保留了 generation 判断，但变化纹理随后经过更长的所有权链：`HostPrimRenderCache` 调用 `RecordingRenderer`，fork 将像素复制进 `HostedSceneOperation`；Astra FVP translator 再复制为 `ScenePacket` resource operation 并计算内容 hash；`PreparedCommit` 进入 postcard payload，随后又随动态 family step 经过 ABI 编解码；`RuntimeWorld` 把 presentation envelope 交给 CLI 后，GPU adapter 再解码并构造平台 scene command。`2fab6d4c` 已移除 GPU adapter 验证用的整包 clone，并让 RGBA create payload 直接转移到 `Arc<[u8]>`，但 fork delta、translator 和动态 ABI 之间的像素复制与序列化仍然存在，不能把轻量 visual identity 误写成“像素没有跨层传递”。

更大的差异在平台资源更新。原版对已有动态纹理原位写入；当前 GPU adapter 为每次 update 分配新 resource generation，先 release 旧 resource，再 upload 新 resource。通用 WGPU scene renderer 发现 retained resource 集合变化后会重新 pack atlas，清零 CPU atlas backing，并上传新的 atlas。一次菜单 hover 或状态切换造成的单纹理变化，因而可能同时触发整张纹理复制、ABI payload 编解码、旧 generation 释放、新 generation 创建和全 atlas 重建。这条链路不满足“局部更新只触碰被修改纹理”的目标。正式修复必须让同尺寸/同格式 update 保持稳定 resource identity，在事务验证后对既有 atlas placement 执行有界 subresource write；只有 create、destroy 或尺寸/格式变化才允许重新布局。

音频也不能照搬 fixed tick。原版 native BGM 使用独立的 streaming playback；当前 adapter 在 `Play` 时把 encoded stream 完整解码、重采样，再由 Runtime fixed tick 上的 pump 维持 120–180 ms 平台队列。图形、ABI 或媒体阶段只要阻塞超过水位，device callback 就会先耗尽队列。音频 producer/queue pump 必须脱离 VM、scene prepare、GPU receipt 和 presentation cadence，在独立有界任务上按低水位补充；Runtime tick 只提交命令和读取有界 telemetry。

实现与审查按下列顺序检查放大点：

1. 先看 `rfvp.core.provider_step`。core p99 正常而 `astra.emu.adapter.effect_dispatch` 出现长帧时，不得把问题归因于 VM。
2. 对每次 scene commit 同时记录 create/update bytes、resource operation、draw count、live generation 和平台 atlas upload bytes。局部 update 后 atlas upload 等于全部 live texture 时，直接判为生命周期放大。
3. 对 payload 分别记录 fork capture、translator、ABI encode/decode 和 GPU prepare 字节数；同一 decoded pixel 不应在单步内拥有多份长期存活副本。
4. 音频 trace 必须区分 command、decode、producer、platform queue、callback underflow。增大 buffer 只能用于明确的 bounded latency policy，不能替代独立 producer。

## 当前性能证据

本地授权样本的一次 clean Release GPU Headless 运行使用同一物理输入，在 DX12 集显 `wgpu_offscreen` 上完成 36,600 个 60 Hz fixed tick 与 73,200 个 120 Hz semantic presentation（其中前 1,200 个是 warmup）。正式 `astra.performance_report.v1` 为 `pass`：Runtime p99 为 2.80 ms、presentation p99 为 0.65 ms、deadline miss 为零，稳态 upload/readback/renderer allocation p95 均为零，memory growth 为零；v3 报告绑定 performance report 与 trace manifest hash。该运行同时产出并可解析 183,000 条 Perfetto Trace Event，trace 丢失和截断均为零，CPU raster phase 的样本数为零。snapshot/restore 正确性由同一输入的独立预跑验证；性能段不在采样窗口执行该昂贵操作。它证明本次 60 FPS baseline 的单轮 E2；不替代三轮 release-reference 对照、10 分钟原生音频 soak、route/media PTS parity 或 Windows Manager E3。

2026 年 8 月 3 日的原生 10 分钟 mixed-run 形成 35,578 个配对 fixed-tick slice，RFVP core p99 为 2.923 ms，fixed tick p99 为 16.690 ms。第 651 step 的 core 为 2.696 ms，effect dispatch 却达到 3.940 s；该 step 正好消费标题菜单转场输入。整段运行的最长 fixed tick 为 5.892 s，后段累计 656 次平台 audio underflow，decoder refill counter 始终为零。证据说明本次撕裂不是 RFVP VM 或增量 decoder 慢，而是 adapter/presentation 长阻塞耗尽了依赖 fixed tick 补充的音频队列。该 run 明确失败，保留为根因 trace，不计入 60 FPS baseline 或音频 soak 通过项。

## 迁移约束

- FVP v5 是 hard cut。v4 snapshot section、逐 syscall journal 和旧 render-frame 都不得进入运行时或保存容器；遇到旧 section 必须返回明确迁移诊断，不能保留双读运行时。
- `na_wmv_player` 与 `na_mpeg2_decoder` 只在没有其他消费者后移除；RFVP hosted-core 不再拥有这些 decoder。
- Headless E2 需要同一 session 的真实 PNG/WAV、artifact manifest、输入消费、state/scene/route/wait/media PTS/audio 签名与视觉审查。单元测试、fixture 或启动日志不能替代它。
