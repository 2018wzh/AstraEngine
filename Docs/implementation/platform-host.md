# Platform Host Blueprint

平台模块只适配原生能力，不拥有 Runtime 权威状态。Migration 8 当前产品化 Windows 与 Chrome Web；Linux 和 macOS 在 Stage 6 进入 `IN_PROGRESS`，iOS、Android 继续使用显式 `Unavailable` factory。

Target 绑定见 [Target And Platform Blueprint](target-platform.md)，native host 状态见 [Migration 8](../migrations/platform-host-migration.md)。平台无关测试后端见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)，生产完备度缺口与收束条件见 [模块能力完备度审查](../migrations/module-completeness-audit-migration.md#p0-004headlessscenariorendereraudio-fixture-和-player-测试仍是分散双轨)。

## Contract

### RFVP media stream path

The Windows PlatformHost owns the Media Foundation `DecodeResource` session.
Video and audio use `DecodeStreamAction::{Start,Next}` with strict sequence,
kind, format, and budget validation. Each resource has its own bounded decode
worker, so `ReadSample` and format conversion never block the Winit command
loop; the host emits one cursor/frame or PCM chunk per request and a stable end
diagnostic, while `Next` never carries encoded bytes. Manager and native CLI
prefetch on bounded workers and close every session during stop, completion,
error, and host shutdown. WMF hardware transforms are requested, while the
explicit CPU BGRA/i16 boundary preserves portable hashing and keeps the final
GPU/device transfer observable. Streaming WMF video uses the metadata-only
`astra.decoded_video_frame_cpu.v1` format and moves the decoder-owned BGRA8
allocation through `DecodeOutput`; it no longer wraps every frame in a postcard
payload. The host still validates the moved buffer hash, dimensions and byte
budget before the consumer's in-place BGRA-to-RGBA conversion. A hardware
transform request is not treated as proof that a particular adapter selected a
hardware transform; that proof remains an explicit release-gate artifact.

### Windowed E2 host path

`astra-emu-cli windowed-e2` is a developer-only conformance path. It creates the
same native Windows window, WGPU device/queue, PlatformHost audio service and
decode workers used by the native launch, but the JSONL file remains the sole
gameplay input source. The input must be the validated
`astra.user_input_sequence.v1` sequence and must end with `Shutdown`; keyboard,
pointer, touch, gamepad, IME and external close events are rejected at the host
boundary and counted instead of entering `RuntimeWorld`. Resize and focus are
lifecycle events only.

The run writes `astra.emu.windowed_e2_report.v1`. The report binds the family
provider and binary, executable build, platform profile, package and entry,
session and input hashes, and records only fixed-step counts, rejected-input
counts, diagnostics and checkpoint frame/observation hashes. Surface readback is
allowed at declared checkpoints only; ordinary frames record present/deadline,
scene/upload, audio queue/refill and resource-lifecycle telemetry. The report is
not an E3 sign-off and cannot be used to claim clean Release performance without
the matching native identity and soak evidence.

```rust
pub trait PlatformHostFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture;
}

pub struct PlatformHostSession {
    pub client: PlatformHostClient,
    pub events: PlatformEventStream,
    pub profile: HostLaunchProfile,
}
```

`HostLaunchProfile::Platform` 只接受 `astra.platform_host_profile.v2`，`HostLaunchProfile::Headless` 接受测试专用 `astra.headless_host_profile.v3`；v2 仅能由普通功能测试显式迁移，性能门禁拒绝迁移结果。`PlatformId` 不增加 Headless variant。native factory 收到 Headless profile 时在 `host.start` 返回 `InvalidProfile`；Headless factory也必须反向阻断 native profile。Release、Cook 和 shipping Player 继续只接收 `PlatformHostProfile`。

`PlatformHostClient` 通过 Future 提交 window/surface/present/capture、audio、decode、save transaction、package range 和 shutdown 命令。OS/browser event loop 在本地主线程 executor 持有 `!Send` 资源，Tokio 只负责编排。

Windows/Linux/macOS host 的命令队列是事件驱动的：命令成功进入有界队列后必须通过 `EventLoopProxy` 唤醒 Winit，并在 `user_event` 中立即排空；队列满、关闭或未成功提交时不得产生伪唤醒。macOS Player 的异步编排运行在独立 Tokio worker，主线程只阻塞在 `pump_app_events(None)`，由 host command/future completion user event 唤醒。HTTPS package completion 同样显式唤醒 event loop。`about_to_wait` 不能作为 render/audio/decode/input command 的生产或补水时钟。Manager 以及 Windows/Linux/macOS host 的 gamepad、metadata 和 translation worker 使用同一类 edge-triggered host wake；Web gamepad source 和媒体完成使用 `requestAnimationFrame`，不再使用固定 interval/timeout。只有底层 native backend 不提供 hotplug handle 时，worker 内部才允许 250 ms 的设备发现等待。唤醒注册重复绑定、event loop 关闭和 queue overflow 都必须输出稳定 diagnostic，不能静默退回 UI fixed polling。

Native fixed-tick loops use the shared absolute-deadline scheduler. Their event
select gives an already-due deadline priority over a burst of host events, so
window/user-event traffic cannot manufacture catch-up debt. A debt diagnostic is
still blocking; it is never repaired by rebasing the clock or silently dropping
ticks. Native CLI/Windowed E2 performs one explicit startup fixed step before
arming the steady-state scheduler: retained atlas/resource creation is measured
as `astra.emu.native_startup_tick`, and an over-budget result is emitted as
`ASTRA_NATIVE_STARTUP_TICK_OVER_BUDGET` rather than being hidden or counted as
a dropped tick. All subsequent steps remain on the absolute deadline and use
the same four-step debt limit.
Native audio callback 的消费/错误边沿也通过共享 `AudioWakeRegistration` 唤醒 drain/refill waiter；绝对 deadline 只作为最终超时边界，禁止用 4/5 ms sleep 模拟补水。队列满、设备丢失、格式漂移、worker panic 和 shutdown drain/abort/join 必须终止受影响 session 并输出稳定 diagnostic。

用户授权原版目录只暴露安全相对路径的 stat/range read。source fingerprint 固定按 4 MiB range 流式读取，同时计算公开文件 SHA-256 与不落盘的私有 key material；单个原版大文件不得通过 whole-file `fs::read` 进入 Player、Headless 或 CLI。每次 range 都校验 offset、length、最大读取量和文件边界，fingerprint 前后再次 stat，长度或内容变化立即阻断。

所有资源使用不可序列化的 `{slot, generation}` typed handle：`WindowHandle`、`SurfaceHandle`、`AudioOutputHandle`、`DecodeSessionHandle`、`MediaFrameHandle`、`SaveTransactionHandle` 与 `PackageSourceHandle`。stale handle、重复 close、越界 range、乱序 completion、队列溢出和 shutdown leak 必须显式报错。

Headless 的 `HttpsRange` source 只接受 allowlist 中不含 credential/fragment 的 HTTPS URL，禁止 redirect 与压缩传输。后端要求严格 `206 Content-Range`，在 open 阶段按有界 block 扫描完整对象并绑定 package hash，read 阶段重新请求并校验覆盖区间的 block identity；不支持 byte range 的服务端直接阻断。

## Crate Split

| Crate | 职责 |
| --- | --- |
| `astra-platform` | profile、typed handle、async command/event contract、capability v2、conformance schema |
| `astra-platform-common` | generational resource table、ordered completion、atomic save、hash-bound package range、shared `WgpuPresentationCore`、audio/gamepad mapper、verified cache 与共用 policy |
| `astra-platform-windows` | winit event loop、hardware wgpu、WASAPI、WMF、Saved Games、Windows package source；test injection 仅在 `platform-test-driver` |
| `astra-platform-web` | canvas/DOM、WebGPU、WebAudio、WebCodecs、OPFS、fetch/File source |
| `astra-platform-headless` | `publish = false` 测试 host 已实现完整 service、物理输入编排和 PNG/WAV artifact；完整 workspace test 与正式 evidence 尚待闭合 |
| `astra-player-web` | 独立 WASM Player，读取 config、package 和 cooked platform profile |
| iOS、Android crate | Stage 6 `PLATFORM_NOT_IMPLEMENTED` factory |

## Platform Profiles

`project.yaml.platform_profiles` 以 `astra.platform_host_profile.v2` 表达 `PlatformHostProfile`。Cook 校验 profile key、target、package、provider policy、package source policy 与 verified package cache 限额，并写入 `platform.profiles` / `astra.platform_profiles.v2` package section。Player 只对既有 v1 section 执行显式迁移；未知 schema blocking，且不接受 CLI 覆盖发布策略。

Windows release 要求 `wgpu_hardware`、`wmf`、`wasapi`、`saved_games`。Web release 只支持 Chrome，固定要求 `webgpu`、`webcodecs`、`webaudio`、`opfs`，不配置 fallback。

Headless 不写入 `project.yaml.platform_profiles` 或 cooked `platform.profiles`。`astra.headless_host_profile.v3` 只供测试 harness 与 Developer 工具使用，声明 provider binding、render policy、JSONL 输入协议、artifact policy、双帧限额、CPU/GPU cache 限额、GPU adapter policy 和 build/package identity。Release Gate、shipping target 或 AstraPlayer 发现该 schema、Headless provider id 或 Developer binary role时必须阻断。

## Reports And Gate

`astra.platform_capability_report.v2` 对 renderer/decode/audio/save 分别记录 `declared`、`available` 和 `selected`。普通 probe 不执行真实设备验收，因此不能仅凭接口存在性把 provider 写入 available。

`astra.platform_host_conformance_report.v1` 绑定 platform、target、profile hash、package hash、build fingerprint、session id 和资源生命周期 checks。Release Gate 还要求 Player automation report 在 profile/package/build/session identity 上连续匹配。

Windows required checks：`host.lifecycle`、`window.create_destroy`、`surface.present_readback`、`input.native_consumption`、`audio.output_meter`、`decode.platform`、`save.atomic_reopen`、`package.hash_range`、`resource.zero_leaks`。

Web required checks：`host.lifecycle`、`window.canvas`、`surface.webgpu_present_readback`、`input.dom_consumption`、`audio.webaudio_meter`、`decode.webcodecs`、`save.opfs_atomic_reopen`、`package.hash_range`、`resource.zero_leaks`。

静态 WAV meter、接口存在性、hidden-window smoke、文件存在、route report、DOM synthetic click 和 `--dump-dom` 只能作诊断，不能通过 `player.full_playable`。

Migration 11 完成后，真实产品平台验收还必须读取 `astra.headless_preflight_link.v2`。Headless 与真实平台 run 绑定同一 build、cooked package、input sequence、scenario、target 和 content identity，但使用各自的 profile/session id。Headless 只形成 E2 证据，不能替代真实平台 E3。
