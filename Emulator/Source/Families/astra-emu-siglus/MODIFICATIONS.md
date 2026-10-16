# Siglus 上游差异

## 固定来源

- 完整 fork：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)，主仓 submodule：`Emulator/ThirdParty/siglus_rs`。
- 上游 HEAD（2026-10-16，1.0.0）：`5f358ef1360b0439e92aed2b749e8f25d8721954`。
- 唯一适配提交：`8bf5024fb3975bb8dc375874380f4574ecedf4d8`，直接父提交为上述 HEAD；分支 `codex/astra-upstream-head-20261016`，本地整合、未发布。
- 来源历史、原生平台与许可证保留；不改写旧分支。

## 核心差异

- `astra-hosted`：Family 关闭默认 feature，接入原 Kira mixer 的 stereo F32 输出；原桌面默认 CPAL 与解码 features 保留，配置限于消费 crate。
- Renderer 的 surface 可选，离屏与窗口共用初始化和 `render_frame_to_view`。保留原 sprite/wipe/shader/Emote/几何及 D3D9 byte-space 合成语义；离屏选择硬件 adapter。
- 原生离屏成功绘制后递增 generation，跨 target resize 连续。Family 无新提交时不回读，不用 VM tick 或像素 hash 代替。
- GPU readback buffer 与完成队列按尺寸持有；当前取帧仍同步等待并创建 CPU Vec，见审计 R04。
- 每 mixer 独立取消，先解除阻塞写再 join；替换 mixer 不取消 session sink。共享注册表使用 parking_lot，诊断复用原 log。
- 成功 load、返回菜单和 restart 增加一次性 reset 通知；原 VM/过程流与实时时钟保留，不启用 replay-only virtual-clock。
- WMV decoder 包版本 0.2.1 仅区分同名路径包，不更换 codec 或移除上游 targets。

## Family 与 Host 边界

Family 负责 ABI、输入、PCM 队列和 CPU lease，调用原 `new_with_renderer_sync`；原 global/config/read 持久化在启动/关闭边界调用 `persist_global_if_changed`，不逐帧 hash 或复制 codec。Manager owner 一次性保留 8 MiB 栈，创建、推进、取帧和关闭保持同线程。

## 结论与验证入口

原生 renderer/Kira 复用和 HEAD＋1 结构成立；同步回读、变化帧分配、受阻关闭及 GPU 警告仍待收敛，**尚未完成性能/长期资源验收**。实际标题、媒体/PCM、原生全局保存与冷重开证据及定向检查统一见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，下一步见[实施计划](../../../../Docs/status/implementation-plan.md)。许可及自备密钥边界见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
