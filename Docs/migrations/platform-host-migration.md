# Migration 8：Windows/Web Platform Host 产品化迁移

**状态：`IN_PROGRESS`**

Migration 8 重新打开原先由 capability smoke 标记完成的平台工作。Windows 与 Web 只有在同一最终 commit、同一 cooked package 和同一次验收 run 中，同时取得 capability v2、host conformance 与 Player automation 连续证据后才能恢复 `DONE`。Linux、macOS、iOS、Android 仍属于 Stage 6。

## 已落地

- `astra-platform` 已改为轻量 contract crate，公开异步 `PlatformHostFactory`、`PlatformHostClient`、事件流及 `{slot, generation}` typed handle；stale handle、重复释放、乱序 completion、队列溢出和 shutdown resource leak 都是显式错误。
- `astra-platform-common` 提供资源表、有序 completion、atomic save transaction、hash-bound package range reader、共享 `WgpuPresentationCore`、lock-free native audio queue、typed gamepad mapper 与 native verified package cache。
- Windows backend 已由 winit 主线程持有窗口和 event loop，接入 shared hardware-only wgpu present/readback、typed input、WASAPI callback meter、WMF decode、Saved Games transaction、rfd 用户授权文件和 verified HTTPS cache；`SendInput` 只存在于 `platform-test-driver` feature，HTTPS 下载在 Tokio worker 完成后回投本地资源线程。
- Web backend 已完成 canvas/DOM event、shared WebGPU present/readback、WebCodecs、WebAudio、OPFS save 及 verified HTTPS package cache；独立 `astra-player-web` WASM crate 会校验 package 和 `platform.profiles` section 后启动 host。
- Linux factory 已进入 Stage 6 `IN_PROGRESS`，普通 probe 仍要求 live conformance，不能把 API presence 写成 provider available；macOS、iOS、Android factory 返回稳定 `PLATFORM_NOT_IMPLEMENTED`。
- capability report 已升级为 `astra.platform_capability_report.v2`，区分 declared、available、selected；release validator 已支持 `astra.platform_host_conformance_report.v1` 与 Player report identity continuity。
- Bundle CLI 要求显式传入已构建的 Windows Player、Crash Reporter、Web WASM、loader 和 AudioWorklet artifact；不再复制当前 CLI，也不再生成 JavaScript route model/runner。
- `project.yaml.platform_profiles` 以 `PlatformHostProfile` 强类型读取，Cook 后写入 `platform.profiles` 自描述 package section。

## 尚未完成

- WebCodecs encoded audio/video chunk decode、OPFS commit/reload/abort、File source、allowlist HTTPS verified cache、visibility/focus/resize/input lifecycle 和 AudioWorklet bounded queue 已接入；OPFS cache 的总量 LRU、真实用户手势后的 WebAudio 正向 conformance、WebGPU device/context loss recovery 与完整 Player runtime route 尚未闭合。
- Windows 用户授权文件、streaming HTTPS verified cache 和 typed gamepad axis 已接入；授权对话框的 Player-window parent binding、缓存打开 handle 的跨进程 LRU lease 与 Player runtime 对全部 host service 的产品接线尚未闭合。
- Windows `SendInput`、focus 与 GDI capture 已迁入 `platform-test-driver` feature，生产 Player 不链接注入 API；Web CDP driver 与真实 Player runtime route 仍待闭合。被旧 JS route runner 支撑的测试已停止作为验收证据。
- 正式 Python acceptance runner、同 run evidence manifest、Windows/Chrome 完整路线与最终 workspace gate 尚未通过。

因此 `migration8`、`S2-PLATFORM-01`、`S2-WINDOWS-HOST-01`、`S2-WINDOWS-GATE-01` 和 `S2-WEB-HOST-01` 当前均为 `IN_PROGRESS`。

## 验收边界

普通 CI 负责 contract、负向门禁、单元测试和 wasm 编译。正式 evidence 必须拒绝 dirty worktree，并绑定 commit、build fingerprint、profile hash、package hash、session id、selected provider 和 check count。静态 WAV meter、接口存在性、hidden-window smoke、route report、DOM synthetic click、`--dump-dom` 或文件存在检查均不能通过 `player.full_playable` 或 Migration 8。
