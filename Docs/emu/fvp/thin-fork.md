# RFVP Fork 与薄 Adapter（Family ABI v9）

本页只描述当前 v9 边界。旧的 v5/v6/v7 hosted delta、scene packet、text
lease、continuation snapshot 和 adapter-side compositor 不属于当前契约。

## 结论

`astra-emu-fvp` 的 AstraEngine 侧已经是薄边界：它只负责 dylib root export、
descriptor/build identity、panic containment 和最终错误映射。正常像素路径应当
只有：

```text
RFVP fork 直接 acquire Host writable lease
    -> RFVP software renderer 写入同一 allocation
    -> RFVP 显式 commit damage
    -> Host 校验并上传
```

当前 pinned RFVP revision 为
`f4f64a5bb726c1759350a666a35e0a454b810f61`。这个 revision 的 provider 已经能
走 `Ported + SingleLayer`、Hook、writable-file 和 surface lease，但 fork 内仍
保留旧的通用 hosted semantic-delta、snapshot/restore 和策略-limit 层，尚未达到
最终 v9 形态。具体证据见 [RFVP fork audit](rfvp-fork-audit.md)。

## 职责边界

RFVP fork 必须拥有：

- FVP VM、syscall、字体 fallback、shaping、换行、布局和最终绘制；
- Host input、wait、audio、video、control DTO 的生成；
- Hook 的同步调用。调用必须发生在 surface acquire 前；失败保留原文并返回
  稳定 diagnostic；
- 一个稳定的 `fvp.main` surface allocation，以及 `Unchanged`、`Full`、像素坐标
  `Rects` damage；
- per-game writable-file Host port 上的原生存档读写。

AstraEngine 的 `astra-emu-fvp` 只能保留：

- `abi_stable` root module 和 dylib 导出；
- plugin descriptor、engine/rustc/feature/fork identity；
- RFVP provider 构造、shutdown、panic containment 和最终 diagnostic boundary。

Adapter 不得出现 framebuffer compositor、scene/draw DTO translator、texture
cache、像素复制或格式重组、text lease/翻译 overlay、snapshot/save envelope、
runtime semantic hash、策略预算或业务状态修补。

## Fork 清理门槛

下列内容必须从 fork 的 shipping hosted surface 移除，而不是通过兼容分支隐藏：

1. `HostedSceneOperation`、texture payload capture、`HostedStepDelta.scene` 和
   adapter-side scene translator。RFVP 可以保留内部的有界 damage bookkeeping，
   但不能把 texture bytes 或 draw list 作为 Host 交易载荷。
2. `HostedSnapshot`、`snapshot_bytes`、`restore_bytes`、semantic state hash 以及
   family continuation snapshot。游戏自己的 save 格式只能经 writable-file port。
3. `HostedTextOperation` 和 ephemeral text storage。Hook 结果必须在 RFVP 内完成
   fallback、shaping、换行和绘制，不把正文转发给 Host。
4. `max_scene_operations`、`max_texture_bytes`、`max_text_operations` 等旧
   policy budget。保留 checked arithmetic、buffer/stride、所有权和实际分配失败
   的 fail-fast 检查；Performance E2 之外的策略预算不参与阻断。

清理完成后，RFVP fork 应更新其 AstraEngine Git dependency 到实际 v9 ABI commit，
独立构建通过后再更新 AstraEngine 的 `Cargo.toml`/`Cargo.lock` pin。

## 验收顺序

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

并完成以下静态检查：

- `cargo tree` 只有一个 `astra-emu-family-api` package identity；
- adapter 中没有 scene、texture capture、像素复制、snapshot、text lease、
  runtime hash 或业务 budget；
- RFVP fork 的独立测试覆盖 writable surface、三态 damage、Hook 原文回退、
  writable-file range/atomic-replace、输入/音频/控制 DTO 和稳定 diagnostic；
- 正常 full-damage 帧只有 RFVP 写 Host lease、Host 上传这一条像素路径。

外部 fork 的 58 个增量提交应在上述清理完成后压成一个可审查提交；不要把 Astra
Engine adapter 的改动混入该提交，也不要通过 force-push 覆盖已有发布分支。
