# RFVP fork v9 职责审计

> 历史记录：本页描述旧 Family ABI v9 分支，不代表当前独立 Host 的实现。2026-09-11 已升级到 RFVP 0.6.0，删除 hosted snapshot/restore 与 canonical state hash API，并接入上游全局持久化。当前差异以 `Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md` 为准。

## 审计结论

结论为 **不通过**。AstraEngine 的 `astra-emu-fvp` adapter 已经足够薄，但
pinned RFVP fork `f4f64a5bb726c1759350a666a35e0a454b810f61` 仍带有一层旧的
通用 hosted adapter。它可以作为当前开发 pin 使用，但不能作为已经完成的
Family ABI v9 release pin，也不能把现有 58 个增量提交直接标记为单一生产提交。

审计基线：

| 项目 | 值 |
| --- | --- |
| RFVP upstream base | `3b5ea6c96a925c12f95aef8554905e8fecbc77c3` |
| hosted fork head | `f4f64a5bb726c1759350a666a35e0a454b810f61` |
| 增量提交数 | 58 |
| 文件差异 | 74 files, `+12,232/-648`（相对 upstream base） |
| AstraEngine dependency | Git revision 固定，但 fork crate 仍把 AstraEngine dependency 写成旧的 `e6bc3d960b87373160acd8507faeac4cc589975b` |

## 已通过的边界

- `crates/rfvp-astra-provider` 集中承载 FVP provider、VFS、surface、Hook、
  writable-file 和 FFI bridge；AstraEngine adapter 不再实现这些数据路径。
- provider descriptor 固定为 `Ported + SingleLayer`。
- provider 的正常路径已经调用 Host surface acquire/commit，且使用
  `OwnedWritableByteBuffer` 往返，不需要 const-cast 或中间像素副本。
- Hook 调用位于 surface acquire 前；失败结果保留原文并产生稳定 diagnostic。
- fork 已有 native save 的 writable-file port，Astra Runtime 不再提供 save slot
  envelope。

## 阻断项

### 1. Hosted semantic delta 仍是实际中间层

`crates/rfvp/src/hosted.rs` 仍公开并维护：

- `HostedSceneOperation`、`HostedTextureData`、`HostedTextureUpdate`；
- `HostedStepDelta.scene` 和 texture payload capture；
- `HostedVisualDamage`、`RecordingRenderer` 以及 operation/copy telemetry；
- `HostedAudioOperation`、`HostedVideoOperation` 和 `HostedTextOperation`。

`crates/rfvp-astra-provider/src/provider.rs` 先消费这些 delta，再把它们转成
Family audio/video/layer DTO。虽然 direct-surface 分支能避免大多数像素复制，
这仍不是“RFVP 直接写 Host lease”的唯一数据路径；旧 capture API 也继续增加
维护和误用面。

### 2. Hosted snapshot 与运行时语义 hash 仍存在

`HostedSession` 仍提供 `snapshot`、`restore`、`snapshot_bytes`、`restore_bytes`、
`canonical_state_bytes` 和 `canonical_state_component_hashes`；底层
`RfvpCore` 仍实现对应 snapshot/hash 方法。Family ABI v9 已删除 snapshot/save/
restore，游戏存档必须只走 writable-file port，这些 API 不能留在 shipping
hosted surface。

### 3. 旧策略 budget 仍暴露

`HostedLimits` 仍包含 scene operation、texture bytes、text operation、audio
operation 和 log limits，并由 `RecordingRenderer` 和 hosted step 强制执行。
保留 checked arithmetic、stride/尺寸、所有权和实际分配失败检查是必要的；旧的
probe/read/prompt/cache/decode 或 scene capture policy budget 不应继续成为 Family
provider 的阻断契约。

### 4. 文档和 dependency identity 未完全同步

fork 的 `ASTRA_HOSTED.md` 仍描述 snapshot、render/audio/video command delta 和
host-neutral hosted-core；同时 `rfvp-astra-provider/Cargo.toml` 仍 pin 到旧的
AstraEngine revision。两者都与 v9 writable surface/Hook 契约不一致。

## 必须完成的 fork 变更

1. 将 hosted renderer 改为只保留内部 damage bookkeeping；删除 scene/texture
   payload capture、`HostedStepDelta.scene` 和 adapter-side translator。
2. 从公开 hosted API 删除 snapshot/restore、canonical state hash 和
   continuation snapshot；保留游戏自己的 native save implementation，仅通过
   writable-file Host port 访问。
3. 将文字 Hook、fallback、shaping、换行和绘制收回 RFVP core；不再通过
   `HostedTextOperation` 或 ephemeral text storage 向 Host 传正文。
4. 删除旧 hosted policy budgets，保留 ABI 表示、checked arithmetic、buffer/
   stride、所有权隔离和实际系统错误的 fail-fast 检查。
5. 将 fork 内所有 AstraEngine Git dependencies 更新到实际 v9 ABI commit，
   独立构建和测试通过后再更新主仓 pin。
6. 在 fork 仓库以一个新提交承载上述清理；不要把 AstraEngine adapter 修改
   混入 fork，也不要 force-push 覆盖已有发布分支。

在这些项目完成前，FVP v9 的 fork thinness gate 必须保持 `BLOCKED`。本次
AstraEngine 提交只固化身份清理和这份审计，不把 fork 的未完成状态伪装成通过。
