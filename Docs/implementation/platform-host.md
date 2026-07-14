# Platform Host Blueprint

平台模块只适配原生能力，不拥有 Runtime 权威状态。Migration 8 当前只产品化 Windows 与 Chrome Web；Linux、macOS、iOS、Android 在 Stage 6 前使用显式 `Unavailable` factory。

Target 绑定见 [Target And Platform Blueprint](target-platform.md)，真实平台迁移状态见 [Migration 8](../migrations/platform-host-migration.md)。平台无关测试后端见 [Migration 11](../migrations/headless-platform-test-backend-migration.md)；该迁移当前只有 `SPEC_READY` 文档，不能作为 host evidence。

## Contract

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

上面的 `HostLaunchProfile` 是 Migration 11 的 planned contract。当前 factory 与 session 仍直接持有 `PlatformHostProfile`。后续类型形状为 `Platform(PlatformHostProfile)` 与 `Headless(HeadlessHostProfile)`；`PlatformId` 继续只包含六个发布平台。native factory 与 Headless factory 必须拒绝错误 variant，shipping API 只能构造 `Platform` variant。

`PlatformHostClient` 通过 Future 提交 window/surface/present/capture、audio、decode、save transaction、package range 和 shutdown 命令。OS/browser event loop 在本地主线程 executor 持有 `!Send` 资源，Tokio 只负责编排。

所有资源使用不可序列化的 `{slot, generation}` typed handle：`WindowHandle`、`SurfaceHandle`、`AudioOutputHandle`、`DecodeSessionHandle`、`MediaFrameHandle`、`SaveTransactionHandle` 与 `PackageSourceHandle`。stale handle、重复 close、越界 range、乱序 completion、队列溢出和 shutdown leak 必须显式报错。

## Crate Split

| Crate | 职责 |
| --- | --- |
| `astra-platform` | profile、typed handle、async command/event contract、capability v2、conformance schema |
| `astra-platform-general` | generational resource table、ordered completion、atomic save、hash-bound package range、shared `WgpuPresentationCore`、audio/gamepad mapper、verified cache 与共用 policy |
| `astra-platform-windows` | winit event loop、hardware wgpu、WASAPI、WMF、Saved Games、Windows package source；test injection 仅在 `platform-test-driver` |
| `astra-platform-web` | canvas/DOM、WebGPU、WebAudio、WebCodecs、OPFS、fetch/File source |
| `astra-platform-headless` | Migration 11 planned、`publish = false` 的完整测试 host；真实执行 surface/audio/decode/save/package/input/artifact lifecycle |
| `astra-player-web` | 独立 WASM Player，读取 config、package 和 cooked platform profile |
| 其余平台 crate | Stage 6 `PLATFORM_NOT_IMPLEMENTED` factory |

## Platform Profiles

`project.yaml.platform_profiles` 以 `astra.platform_host_profile.v2` 表达 `PlatformHostProfile`。Cook 校验 profile key、target、package、provider policy、package source policy 与 verified package cache 限额，并写入 `platform.profiles` / `astra.platform_profiles.v2` package section。Player 只对既有 v1 section 执行显式迁移；未知 schema blocking，且不接受 CLI 覆盖发布策略。

Windows release 要求 `wgpu_hardware`、`wmf`、`wasapi`、`saved_games`。Web release 只支持 Chrome，固定要求 `webgpu`、`webcodecs`、`webaudio`、`opfs`，不配置 fallback。

Headless 不写入 `project.yaml.platform_profiles` 或 cooked `platform.profiles`。planned `astra.headless_host_profile.v1` 只供测试 harness 与 Developer 工具使用，声明 provider binding、JSONL 输入协议、artifact policy、限额和 build/package identity。Release Gate、shipping target 或 AstraPlayer 发现该 schema、Headless provider id 或 Developer binary role时必须阻断。

## Reports And Gate

`astra.platform_capability_report.v2` 对 renderer/decode/audio/save 分别记录 `declared`、`available` 和 `selected`。普通 probe 不执行真实设备验收，因此不能仅凭接口存在性把 provider 写入 available。

`astra.platform_host_conformance_report.v1` 绑定 platform、target、profile hash、package hash、build fingerprint、session id 和资源生命周期 checks。Release Gate 还要求 Player automation report 在 profile/package/build/session identity 上连续匹配。

Windows required checks：`host.lifecycle`、`window.create_destroy`、`surface.present_readback`、`input.native_consumption`、`audio.output_meter`、`decode.platform`、`save.atomic_reopen`、`package.hash_range`、`resource.zero_leaks`。

Web required checks：`host.lifecycle`、`window.canvas`、`surface.webgpu_present_readback`、`input.dom_consumption`、`audio.webaudio_meter`、`decode.webcodecs`、`save.opfs_atomic_reopen`、`package.hash_range`、`resource.zero_leaks`。

静态 WAV meter、接口存在性、hidden-window smoke、文件存在、route report、DOM synthetic click 和 `--dump-dom` 只能作诊断，不能通过 `player.full_playable`。

Migration 11 完成后，真实产品平台验收还必须读取 `astra.headless_preflight_link.v1`。Headless 与真实平台 run 绑定同一 build、cooked package、input sequence、scenario、target 和 content identity，但使用各自的 profile/session id。Headless 只形成 E2 证据，不能替代真实平台 E3。
