# RFVP Thin Fork 与 Hosted Adapter

## 目的与当前状态

FVP 采用 `2018wzh/rfvp` 的 `astra-hosted` 分支作为小型、可重放的 fork。它只补充 host-neutral `hosted-core`，不把 Astra 类型、RuntimeWorld、序列化格式、错误码、路径约定或平台 GPU/audio handle 写入 RFVP。

截至本文更新，fork 已固定 upstream base，并已加入有界 `HostedSession`、`HostedStepInput`、`HostedStepDelta`、session-owned globals、内存 snapshot/restore、Shipping/Evidence 固定 trace ring、`.bin` metadata 后的按需 range-read，以及仅含 URI/长度的视频资源 delta。Astra 的 FVP adapter 已具备 session-owned `ScenePacket` translator：它将 create/partial-update/destroy 转为有界资源操作，先通过 `PreparedCommit` 验证完整事务再替换元数据；旧 render-frame 转换仍只用于过渡测试。动态 FVP provider、Manager/CLI renderer 消费和 Headless E2 尚未切换，本页不是 E2 或性能完成声明。

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

- Shipping 只传 scene/resource/media 的语义 delta；不得逐 opcode 分配 trace、格式化 opcode 字符串、序列化完整状态或复制完整 RGBA framebuffer。
- Evidence 使用固定容量 crash trace ring 和显式 profile。它是受限诊断，不得改变 Shipping 执行、状态 hash 或资源访问。
- 纹理按 id/generation 管理：创建、局部更新、销毁均为显式操作；adapter 在资源/profile/binding 检查完成前不得提交部分帧。
- `.bin` 只读取受限 metadata；entry 通过受限 range-read 提供。禁止启动时预载整包，也禁止把商业 bytes 写入 save、replay、日志或报告。
- `PreparedCommit` 在 host 完成 ABI、资源、预算、hash、profile 与 binding 验证后才可提交。任何缺失或不匹配都必须阻断。

## 迁移约束

- FVP v5 是 hard cut。旧 v4 save/replay 仅返回明确迁移诊断，不能保留双读运行时。
- `na_wmv_player` 与 `na_mpeg2_decoder` 只在没有其他消费者后移除；RFVP hosted-core 不再拥有这些 decoder。
- Headless E2 需要同一 session 的真实 PNG/WAV、artifact manifest、输入消费、state/scene/route/wait/media PTS/audio 签名与视觉审查。单元测试、fixture 或启动日志不能替代它。
