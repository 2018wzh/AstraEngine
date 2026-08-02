# RFVP Thin Fork 与 Hosted Adapter

## 目的与当前状态

FVP 采用 `2018wzh/rfvp` 的 `astra-hosted` 分支作为小型、可重放的 fork。它只补充 host-neutral `hosted-core`，不把 Astra 类型、RuntimeWorld、序列化格式、错误码、路径约定或平台 GPU/audio handle 写入 RFVP。

截至本文更新，fork 已固定 upstream base，并已加入有界 `HostedSession`、`HostedStepInput`、`HostedStepDelta`、session-owned globals/text、snapshot/restore、canonical state identity、最多 64 MiB 的 opaque snapshot bytes、Shipping/Evidence 固定 trace ring、`.bin` metadata 后的按需 range-read，以及仅含 URI/长度的视频资源 delta。Astra 的注册 case image 和动态 host VFS 都经无平台 handle 的 hosted VFS/clock port 打开；动态 provider 每 tick 只接收一个 hosted delta，转换为 `ScenePacket`、媒体命令、local-only text lease 和 `PreparedCommit`。named audio 在 fork 中只排入 source URI，资源字节由 adapter 后续经 session-bound host VFS 读取；RFVP core 不再通过内部或进程 VFS 读取这类资源。`ScenePacket` translator 将 create/partial-update/destroy 转为有界资源操作，先验证完整事务再替换元数据；restore 后的纹理重建显式开始新的资源 epoch，Manager/WGPU 与 CLI CPU reference 都先清除旧资源、独立复验 commit，再写入各自资源存储；solid draw 使用保留的白色 sentinel texture，不借助平台 handle。`astra-emu-fvp` 已不再编译依赖本地 RFVP vendor core。旧 render-frame、逐 syscall journal 和逐 opcode 字符串 trace 不再参与 v5 provider。

本地公开 Win95 Painter sample 的 signed dynamic FVP v5 已通过 120 fixed step 的 Headless run：6 条物理输入均被 host 消费、4 个实际 CPU frame、一个 PNG checkpoint、VFS 2 资源/20 次 range-read、snapshot round-trip 和正常 host shutdown 均通过；人工查看 checkpoint，窗口、工具栏、调色板、画布与底部状态栏可见且无残缺。该输入序列未产生可见笔划，因此它只证明 input transport，不证明脚本交互语义。CPU reference 该次 step p95 为 11.22 ms，4 次 raster 的中位数为 248.12 ms；它用于确认 scene dedup 没有退化为逐 tick 全帧光栅化，不是 GPU 或 RFVP 对比结论。该 sample 无音频、未到脚本 terminal，也没有媒体、路线、性能 soak 或 Windows E3，所以只构成 hosted-v5 的局部 Headless E2/视觉证据，不能作为完成声明。

另一个由公开无资源脚本生成的 signed dynamic FVP v5 Headless lifecycle case 已在 2 step 达到 `terminal`，并同时通过 snapshot round-trip、PNG checkpoint、正常 session shutdown 与 host shutdown。该 checkpoint 是预期的空黑 frame，只证明 `ExitMode(3)` 到 hosted terminal 的生命周期传播，不是产品视觉、路线或媒体证据。

公开生成的 input case 将主按钮 edge 送入 hosted session，并在下一固定 step 将一个已分组的 64×64 tile 从黑色改为红色；其 3 step signed dynamic run 同时通过 snapshot round-trip、PNG checkpoint、terminal 与 shutdown。人工检查 checkpoint，左上角 tile 为红色且其余画面保持黑色。这是 physical input → VM → `ScenePacket` → CPU reference 的可见 E2，证明输入 transport 和语义提交；它不替代实际游戏路线、文本、媒体或平台 E3。

公开生成的 audio case 以 `AudioLoad`/`AudioPlay` 请求一个 Ogg source URI，并在四个 `ThreadNext` 后停止；其 62 step signed dynamic run 通过 snapshot round-trip、checkpoint、shutdown 与 VFS resource policy，输出 49,600 个音频 frame 和两个真实 WAV artifact，audio meter hash 非空。它覆盖 RFVP named-audio → URI-only hosted delta → adapter session resource read → Headless decoder/output 的链路；这是局部媒体 E2，不等同于真实游戏音频、视频、PTS/route 或 Windows E3。

2026-08-02 的授权本机安装 smoke 以签名动态 v5 family 完成 300 fixed step：170 个 scene commit/raster frame、两个 PNG checkpoint、同 session snapshot round-trip 和正常 shutdown 均通过，VFS 记录 14 个资源、55,011 次受限读取、41,648,158 bytes，且没有 blocking diagnostic。人工查看后一个 checkpoint，标题画面完整可见；前一个启动期 checkpoint 为空白，未被当作视觉通过项。该 run 没有路线输入、terminal、音频或视频证据，CPU reference 的 step p95 为 58.08 ms，只是 Evidence profile 下的本机趋势，不能视为性能放行、媒体 parity 或 Windows E3。

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

1. 在 fork 分支上逐个提交可审查 patch，标题使用 `[hosted]`。
2. 每个 patch 必须只修改相邻 RFVP 模块，并在提交前记录 upstream base、patch id、许可证来源和验证命令。
3. Astra 的 Cargo dependency 只钉住已推送的 git revision；禁止再次 vendor RFVP 或使用浮动 branch/tag。
4. 更新 upstream 时先在 fork rebase，运行 upstream 回归和 hosted-core 测试，再更新 Astra pin。不得把 Astra adapter 修改混入 fork rebase。

## 性能与正确性原则

- Shipping 只传 scene/resource/media 的语义 delta；不得逐 opcode 分配 trace、格式化 opcode 字符串、序列化完整状态或复制完整 RGBA framebuffer。接收端以尺寸、draw list 和已验证资源内容 hash 计算轻量 visual identity，再校验并提交 `PreparedCommit`；不得为了帧去重再次序列化包含纹理像素的 commit。
- Evidence 使用固定容量 crash trace ring 和显式 profile。它是受限诊断，不得改变 Shipping 执行、状态 hash 或资源访问。
- 纹理按 id/generation 管理：创建、局部更新、销毁均为显式操作；adapter 在资源/profile/binding 检查完成前不得提交部分帧。
- `.bin` 只读取受限 metadata；entry 通过受限 range-read 提供。禁止启动时预载整包，也禁止把商业 bytes 写入 save、replay、日志或报告。
- named hosted audio 保留 source URI 并转换为受 policy 约束的资源命令；没有 source identity 的 encoded bytes 不能伪装成 URI 或跨 ABI 传递，必须以 blocking diagnostic 停止提交。PCM stream command 仍可在既有限额内转换。
- `PreparedCommit` 在 host 完成 ABI、资源、预算、hash、profile 与 binding 验证后才可提交。任何缺失或不匹配都必须阻断。

## 迁移约束

- FVP v5 是 hard cut。v4 snapshot section、逐 syscall journal 和旧 render-frame 都不得进入运行时或保存容器；遇到旧 section 必须返回明确迁移诊断，不能保留双读运行时。
- `na_wmv_player` 与 `na_mpeg2_decoder` 只在没有其他消费者后移除；RFVP hosted-core 不再拥有这些 decoder。
- Headless E2 需要同一 session 的真实 PNG/WAV、artifact manifest、输入消费、state/scene/route/wait/media PTS/audio 签名与视觉审查。单元测试、fixture 或启动日志不能替代它。
