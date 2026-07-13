# AstraVN Live Player Automation

本页定义 Stage 3 live player automation 的设计边界。目标不是再跑一次 headless scenario，也不是让 route runner 代替玩家；Stage 3 的 Windows/Web 可玩验收必须证明真实 player host 收到平台输入、渲染发生变化、音频图有输出，并且 route 由同一次 player run 推进。

## 证据链

`astra.player_route_report.v1` 仍保留为 bundle route slice：它能证明 player 入口读取 bundle manifest、package、scenario refs、route model 或 mount policy，并输出脱敏 route report。它不能单独满足 `player.full_playable`。

`player.full_playable` 需要同一次自动化运行产出三个 report：

| Schema | 作用 | 允许记录 |
| --- | --- | --- |
| `astra.player_automation_script.v1` | 描述启动、点击、按键、等待、截图采样、音频采样和期望 route/system UI 状态 | target/profile/platform、公开 scenario 相对路径、target region id、按键名、等待条件和期望 check id |
| `astra.player_input_transcript.v1` | 记录每个真实平台输入事件和 player event loop 接收情况 | event source、坐标、按键、target region、frame hash before/after、focus state、event-loop receipt、diagnostic |
| `astra.player_automation_report.v1` | 聚合 input transcript、visual report、audio report 和 route report | check id/status、region hash、meter summary、host evidence id、route coverage 和 blocking diagnostic |

这些 report 不得记录截图、音频、影片、商业正文、商业素材 payload、本地绝对路径、用户名、native handle 或 host secret。视觉证据只能写 region id、尺寸、frame hash 和 changed/blank 状态；音频证据只能写 bus id、peak/rms 区间、sample count、host provider evidence 和 silent/blocking 状态。

## Shared Player Core

当前已新增 `astra-player-core` 的 automation script/transcript/report schema 与 validator，以及 `astra-player` 的 Windows/Web report 校验入口；它们能阻断 direct `route_scenario`、DOM click、JS callback 和直接 `VnPlayerCommand` 冒充 live input。后续平台 host 实现仍需要加载 `.astrapkg`、运行 `VnRuntime`、维护 `PlayerScene`、处理 `PlayerPlatformEvent`，并输出 route/visual/audio/input report。live-player gate 只能调用 `apply_platform_event(event: PlayerPlatformEvent)` 一类平台事件入口，不能暴露测试专用 `advance()`、`choose()`、`open_system()` 或直接 `VnPlayerCommand` 快捷通道。

平台事件进入 core 后必须走与真实 player 相同的 event loop：

1. platform driver 发现并聚焦 player host。
2. driver 注入 mouse/keyboard/touch input。
3. host event loop 把输入转换为 `PlayerPlatformEvent`。
4. `astra-player-core` 推进 VN runtime、presentation、audio graph 和 route state。
5. 同一次 run 采样 frame/audio/route/input evidence。

`VnPlayerCommand` 可继续服务 headless scenario 和 crate 单元测试，但不得出现在 live-player gate 的输入路径或 evidence 中。

## Headless Preflight

Migration 11 将新增平台无关 `astra.user_input_sequence.v1`。它只包含 focus/resume、keyboard、IME、pointer、wheel、touch、gamepad、固定时间推进、await、checkpoint 和 shutdown；不允许 `advance`、`choose`、`open_system` 或直接 `VnPlayerCommand`。当前 `VnPlayerCommand` 测试路径在迁移完成前仍是既有局部测试能力，不能成为新 Headless backend 的最终入口。

产品、Player、样例和 full-playthrough 在进入 Windows/Web live automation 前，必须先用相同 build、cooked package 和 input sequence 通过 Headless 自动比较与模型审查。`astra.headless_preflight_link.v1` 只负责绑定两次 run，不能把 Headless 的 PNG/WAV、route 或 input transcript冒充 Windows/Web host evidence。

## Windows Driver

Windows Stage 3 driver 必须启动真实 player 窗口，发现目标窗口并确认 focus。鼠标和键盘输入必须由 Win32 `SendInput` 发送到 player 窗口；输入后必须证明 winit event loop 收到对应 mouse/keyboard event，再由 runtime 推进 dialogue、choice、system page、config、save、load 和 backlog 路径。

Windows required evidence：

| Check | Required evidence |
| --- | --- |
| `player.window.focused` | player window focus state before input |
| `player.input.sendinput.mouse` | `SendInput` mouse event, target region and event-loop receipt |
| `player.input.sendinput.keyboard` | `SendInput` keyboard event and event-loop receipt |
| `player.visual.window_regions` | real window capture or renderer readback, nonblank and changed region hash |
| `player.audio.wasapi_meter` | AudioGraph meter plus WASAPI host evidence from the same run |
| `player.route.full` | route/system UI checks reached through platform input |

## Web Driver

Web Stage 3 driver 可以继续用本地 HTTP server 承载 bundle，但必须打开真实 Chrome/Edge 页面并通过 Chrome DevTools Protocol 注入输入。鼠标、键盘和必要的触摸输入分别使用 `Input.dispatchMouseEvent`、`Input.dispatchKeyEvent` 和 `Input.dispatchTouchEvent`。禁止使用 DOM `element.click()`、直接调用 app JS API、直接写 route state 或 `--dump-dom` route runner。

Web required evidence：

| Check | Required evidence |
| --- | --- |
| `player.browser.cdp_session` | browser process/page/CDP session established |
| `player.input.cdp_mouse` | CDP mouse event, target region and app event receipt |
| `player.input.cdp_keyboard` | CDP keyboard event and app event receipt |
| `player.visual.canvas_regions` | canvas/browser screenshot region hash, nonblank and changed |
| `player.audio.webaudio_meter` | WebAudio meter from the same player run |
| `player.route.full` | route/system UI checks reached through CDP input |

CDP transport由 `astra-player::WebCdpSession` 持有，连接只允许本机 `ws` page target。它负责 request/response sequence、timeout、duplicate/invalid message blocking、`Runtime.exceptionThrown` blocking、`Input.dispatchMouseEvent`、`Input.dispatchKeyEvent`、`Page.captureScreenshot` 和 `ASTRA_PLAYER_EVIDENCE ` console envelope 解析。启动按钮和 canvas 只允许读取固定 selector 的几何位置后使用 CDP input；driver 不得调用 `element.click()`、产品 JS callback、route runner 或修改页面状态。

## Blocking Rules

`player.full_playable` 必须在以下情况 blocked：

- 缺 `astra.player_input_transcript.v1`。
- 输入 transcript 没有平台事件来源，或事件没有被 player event loop 接收。
- Windows 没有 focus、`SendInput` mouse 或 `SendInput` keyboard evidence。
- Web 没有 CDP session、CDP mouse 或 CDP keyboard evidence。
- 截图、canvas 或 window region 为空，或输入前后 region hash 没变化。
- required voice/BGM/SE 期望存在但 AudioGraph、WebAudio 或 WASAPI meter 静音。
- 缺 Windows/Web host evidence。
- 发现 `VnPlayerCommand`、`--route-scenario` 自推进、`--dump-dom` route runner、DOM `element.click()`、JS callback 或直接 runtime command path。

`input.browser`、`input.gamepad`、`input.touch`、`input.ime` 这类 API 可用性只能说明 capability 存在，不能作为可玩证据。

## Stage 6 扩展

Stage 3 只要求 Windows 和 Web。Linux、macOS、iOS 和 Android 的同类平台输入自动化进入 Stage 6：桌面平台需要真实窗口、focus、原生输入、window/canvas frame evidence 和 host audio meter；移动平台需要设备或模拟器上的 touch/keyboard、safe area、resume、audio session 和 package source evidence。Stage 6 之前不得把这些平台的 capability smoke 写成 Stage 3 `DONE` 证据。
