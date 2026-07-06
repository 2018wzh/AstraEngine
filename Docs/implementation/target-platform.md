# Target And Platform Blueprint

Target 是可执行产品形态，Platform 是运行宿主能力。二者共同决定 package、profile、SDK、Release Gate 和测试入口；Runtime、Editor、CLI、AstraEMU 都不能私自解释平台。

## Target Model

`astra-target` 是 Target schema 真源：

```rust
pub enum TargetKind {
    Editor,
    Game,
    Program,
    Client,
    Server,
}

pub struct TargetDescriptor {
    pub id: String,
    pub kind: TargetKind,
    pub crate_name: Option<String>,
    pub binary: Option<String>,
    pub default_profile: Option<String>,
    pub platforms: Vec<String>,
    pub packaged: bool,
}
```

v1 实现 `Editor`、`Game` 和 `Program`。`Client`、`Server` 是保留 schema 值，进入 manifest 会产生 warning，不作为当前 release gate 的通过条件。

`project.yaml` 直接声明 Target：

```yaml
schema: astra.project.v1
id: com.example.nativevn
runtime: astra-vn
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    platforms: [windows, linux, macos, ios, android, web]
    packaged: true
```

Cook 读取 `targets` 并写入 `cook_manifest.yaml`，用于记录构建时可选的 Editor、Game 和 Program target。Package build 只能打包已经 cook 过的同一个 target，并把 `target.manifest` section 收敛成一个已选择的 packaged `Game` target；Editor 和 Program descriptor 不进入 `.astrapkg`。缺失 Target 时，兼容路径会从 project id 生成一个 Game target；产品级样例必须显式写出 Target。

## Target Rules

| Kind | 可打包 | 必填 | 禁止 |
| --- | --- | --- | --- |
| `Game` | yes | `default_profile`、`platforms` | Editor UI、platform native handle、Developer-only crate |
| `Editor` | no | `binary`、desktop platforms | packaged runtime dependency |
| `Program` | no | `binary` | creator-only state 写入 package |
| `Client` / `Server` | no | 无当前 release gate | 当前 Stage 标为完成 |

`astra target validate <project> --target <id>` 输出 `astra.target_validation_report.v1`。Release Gate 读取 package 内的 `target.manifest`，使用 `target.manifest` check 阻断缺失、重复 id、不是单一 packaged Game、Game 不可打包或选择了不存在的 Target。

## Platform Model

`astra-platform` 是平台 host schema 真源：

```rust
pub enum PlatformId {
    Windows,
    Linux,
    Macos,
    Ios,
    Android,
    Web,
}

pub enum SdkStatus {
    Present,
    Missing,
    Unknown,
}

pub struct PlatformCapabilityReport {
    pub schema: String,
    pub platform: PlatformId,
    pub target: Option<String>,
    pub sdk_status: SdkStatus,
    pub renderer: Vec<String>,
    pub decode: Vec<String>,
    pub audio: Vec<String>,
    pub filesystem: Vec<String>,
    pub input: Vec<String>,
    pub lifecycle: Vec<String>,
    pub permissions: Vec<String>,
    pub smoke: Vec<PlatformSmokeCheck>,
}
```

每个平台有独立 host crate：

| Platform | Crate | SDK 判定 | 必备能力 |
| --- | --- | --- | --- |
| Windows | `astra-platform-windows` | host Windows | winit window smoke、WMF、WASAPI、Known Folder save store、IME、gamepad |
| Linux | `astra-platform-linux` | host Linux | planned winit/wgpu、GStreamer or FFmpeg profile、XDG data、IME |
| macOS | `astra-platform-macos` | host macOS | planned Metal、AVFoundation、CoreAudio、AppKit lifecycle |
| iOS | `astra-platform-ios` | iOS target or Apple developer env | planned Metal、AVFoundation、safe area、touch、no-JIT Luau |
| Android | `astra-platform-android` | Android SDK env | planned Vulkan、MediaCodec、SAF、touch、no-JIT Luau |
| Web | `astra-platform-web` | wasm32 browser environment | WebGPU/WebGL、WebCodecs、WebAudio、OPFS/IndexedDB、File API/fetch package source |

缺 SDK 的平台必须输出 `sdk_status: missing`，Release Gate 对真实平台完成项判为 blocked。`sdk_status: present` 还必须附带该平台 required smoke：Windows 当前要求 `windowed_smoke`、`decode.wmf` 和 `save.known_folder`；Web 当前要求 `browser_smoke`、`renderer.webgpu_or_webgl`、`decode.webcodecs`、`audio.webaudio_unlock`、`save.web_storage` 和 `package.web_source`。Linux、macOS、iOS 和 Android required smoke 已登记为计划项，代码未实现前不能标为 `DONE`。普通 CI 可以验证 schema、report 和 CLI，不把缺 SDK 或缺 smoke 平台标成已完成。

## CLI And Gate

```bash
astra target list project.yaml
astra target validate project.yaml --target nativevn-game --format json
astra platform probe --platform windows --target nativevn-game --format json
astra cook project.yaml --profile desktop-release --target nativevn-game --out target/cooked
astra package build target/cooked --target nativevn-game --out target/nativevn.astrapkg
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml
```

Report 输出只包含 schema、target id、platform id、capability、SDK 状态和 diagnostic code，不记录本地绝对路径、payload body、secret 或 native handle。
