# Artemis 上游差异

## 固定来源

- 完整 fork：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，主仓 submodule：`Emulator/ThirdParty/art3m1s-core`。
- 上游 HEAD（2026-10-16，0.4.0）：`ad90a62cb192b119b7cf8589aa7f87a94bd747c8`。
- 唯一适配提交：`b802ab8eb397d12c623df793ad9feb8d3ae0b5a9`，直接父提交为上述 HEAD；分支 `codex/astra-upstream-head-20261016`，本地整合、未发布。
- 来源历史、Lua 5.1、NativeVulkan/NativeMetal、文字、FFmpeg 9 与许可证保留。

## 差异分类

| 类别 | 当前内容 | 处置 |
| --- | --- | --- |
| 输出钩子 | fallible `try_advance_frame` / `try_capture_frame_into`，推进与捕获分离 | 保留静态跳过与明确错误，FrameLease/代次留在 Family |
| 生命周期 | 视频音频提取归播放实例所有；停止/替换/关闭取消并 join，清理临时 WAV | 保留必要边界，与通用 AVIO 修复分开 |
| 功能修复 | PF8 路径分隔符、AVIO 释放与所有权、packed stereo sample、onLoad 等待归属 | 审计 R03：独立上游处理，不统称嵌入钩子 |
| 产品策略 | 原生公共 Vulkan 选择排除 CPU/未知 adapter | 审计 R06：移到显式嵌入配置，保持上游默认行为 |
| 整合缺口 | 成功恢复未传播 reset_clock | 审计 R07：补一次性成功通知，不改保存格式 |
| 构建闭合 | 独立 workspace；可选原作者桥使用精确 revision | 保留真实依赖，不改为相邻 Astra Family 的另一运行路径 |

可选桥 pin：`Alphaly2K/rfvp@ec204312e123b4839cec8e69e6237fb0374e5518`、`Alphaly2K/siglus_rs@28917920a3e5eeb9a5fa60eac31400422cd41fe2`。AstraEMU 不启用这两条桥；后者实际提供 `art3m1s-host-audio`。等价的 FFmpeg 版本范围文字变化应去掉。

## Family 与上游修复

Family 使用原 GPU/FFmpeg session，因核心无 PCM mixer，复用 SDK Symphonia/Kira 处理原音频命令与独立影片通道。CPU 输出采用三个有界槽和不可变 lease；原生 BGM crossfade、未支持 UI 请求保持明确错误。

onLoad 等待修复已独立提交 [issue #4](https://github.com/Alphaly2K/art3m1s-core/issues/4)、[PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)，提交 `ad20b8afd01ce2807510fa4e08629afc8e62345b` 仅含原生顺序修正、回归与 CHANGELOG。当前等待合并，产品 gitlink 不变。

## 结论与验证入口

**尚未通过严格最小适配验收。** 当前商业 headless 在 typed 窗口请求处明确拒绝；非商业 GPU/媒体验证和独立 PR 的失败前/通过后证据统一见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，执行顺序见[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
