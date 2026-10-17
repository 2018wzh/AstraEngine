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

配置保留 `archive` 文件选择。CPU 捕获沿用原生完整 stage target，报告实际 L=R；窗口 drawable 变化不改原生尺寸。指针按正在显示帧的会话/输出代次校验，再执行一次 raster→logical 映射。读取成功的存档通知换代，旧输出不再接受输入，但旧租约可在 close 后任意线程释放。

原生修复分别提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)（onLoad，`ad20b8afd01ce2807510fa4e08629afc8e62345b`）、[PR #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)（PF8，`186ccc17fb10a6c6b3ee931d98a7c9d6830e722e`）及 [PR #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)（AVIO，`348b931068d31bd75eb5ec804967ce4b86448aee`）。最新远端核对均为 open、未合并。packed stereo 使用显式 `(i16, i16)`；同一非对称双声道样本在原 scalar 和 tuple 路径均正确，不将其报告为已证实缺陷。

## 结论与验证入口

既有基线曾使用原生媒体启用 API，自然播放至ケロQ《終ノ空remake -2025ver-》Windows 版的 1600×1200 标题；该结果不作为本轮 C ABI 验收。旧商业 headless 在 typed 窗口请求处拒绝，新的产品窗口/存读档须重新验收。严格最小钩子内容仍待上游修复合并，基线证据见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，执行顺序见[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

本轮实现尚未执行构建或运行检查，由整合者统一执行。独立硬件 smoke（真实 GPU/音频设备，合成 PF6 作品，不代表商业视听验收）：

```sh
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-artemis --test lifecycle -- --ignored --test-threads=1
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-artemis --lib native_device_completion_pause_cancel_reopen_and_decode_failure -- --ignored --test-threads=1
```

该组覆盖真实 native 创建/自主产帧、pause/resume、取消/关闭/同进程重开、跨线程归还 close 后租约及原生 save/load 代次失效。商业作品使用 `astra-emu-manager --headless <私有配置.json>`，`reopen_count` 保持 DLL 驻留，`frames` 是观察样本而非 VM tick；必须保留截图与 `.diagnostics`，原生 UI 不支持错误不能跳过。
