# Artemis 上游差异

## 固定来源

- 完整 fork：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，主仓 submodule：`Emulator/ThirdParty/art3m1s-core`。
- 上游 HEAD（2026-10-17 核对，0.4.0）：`ad90a62cb192b119b7cf8589aa7f87a94bd747c8`。
- 唯一适配提交：[`ae967d8446fe99b97f6e1df93fb1b61d8ada2040`](https://github.com/2018wzh/art3m1s-core/commit/ae967d8446fe99b97f6e1df93fb1b61d8ada2040)，直接父提交为上述 HEAD；分支 `codex/astra-minimal-native-20261017` 已推送 GitHub，远端 SHA 与主仓 gitlink 一致。
- 来源历史、Lua 5.1、NativeVulkan/NativeMetal、文字、FFmpeg 9 与许可证保留。

## 差异分类

| 类别 | 当前内容 | 处置 |
| --- | --- | --- |
| 输出钩子 | fallible `try_advance_frame` / `try_capture_frame_into`，推进与捕获分离 | 保留静态跳过与明确错误，FrameLease/代次留在 Family |
| 生命周期 | 视频音频提取归播放实例所有；停止/替换/关闭取消并 join，清理临时 WAV | 保留必要边界，与通用 AVIO 修复分开 |
| 原生功能修复 | PF8 路径分隔符、AVIO 释放与所有权、onLoad 等待归属 | 独立 PR #5/#7/#9 均已复现失败前/通过后，等待上游合并 |
| 产品策略 | 显式 `NativeVulkanHardware` 选择 Discrete/Integrated/Virtual，拒绝 CPU/未知 adapter | 原 `NativeVulkan`/`PlatformDefault`、FFI 映射及默认设备排序保留 |
| 恢复通知 | 完整成功 load/reset 后生成一次性通知，Family 清 remainder 并返回 reset_clock | 失败不产生新通知，也不清除此前未消费的成功通知；保存格式不变 |
| 构建闭合 | 独立 workspace；可选原作者桥使用精确 revision | 保留真实依赖，不改为相邻 Astra Family 的另一运行路径 |

可选桥 pin：`Alphaly2K/rfvp@ec204312e123b4839cec8e69e6237fb0374e5518`、`Alphaly2K/siglus_rs@28917920a3e5eeb9a5fa60eac31400422cd41fe2`。AstraEMU 不启用这两条桥；后者实际提供 `art3m1s-host-audio`。FFmpeg 版本声明已恢复 `9.0.0`，解析版本不变。

## Family 与上游修复

Family 使用原 GPU/FFmpeg session，因核心无 PCM mixer，复用 SDK Symphonia/Kira 处理原音频命令与独立影片通道。CPU 输出采用三个有界槽和不可变 lease；原生 BGM crossfade、未支持 UI 请求保持明确错误。

原生修复分别提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)（onLoad，`ad20b8afd01ce2807510fa4e08629afc8e62345b`）、[PR #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)（PF8，`186ccc17fb10a6c6b3ee931d98a7c9d6830e722e`）及 [PR #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)（AVIO，`348b931068d31bd75eb5ec804967ce4b86448aee`）。最新远端核对均为 open、未合并。packed stereo 使用显式 `(i16, i16)`；同一非对称双声道样本在原 scalar 和 tuple 路径均正确，不将其报告为已证实缺陷。

## 结论与验证入口

显式硬件策略、成功恢复通知及原生功能修复已实现。独立原生功能控制组使用现有媒体启用 API，自然播放至ケロQ《終ノ空remake -2025ver-》Windows 版的 1600×1200 标题；启动文件为 `tsuinosora_remake2025ver.exe`，EXE 未提供版本号。商业 Family headless 在 typed 窗口请求处明确拒绝，产品窗口/存读档验收须在 Sandbox 完成。严格最小钩子内容仍待上游修复合并，证据与环境阻塞见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，执行顺序见[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
