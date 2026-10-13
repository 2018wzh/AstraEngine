# AstraVN Live Player Automation

本页解释已有 live-player automation 与报告边界，不另规定 Stage 顺序。真实可玩验证必须观察 Player event loop 的平台输入、画面变化、设备音频与同次运行的路线；Headless 局部场景或 route runner 不能替代。

## 证据链

`astra.player_route_report.v1` 仍保留为 bundle route slice：它能证明 player 入口读取 bundle manifest、package、scenario refs、route model 或 mount policy，并输出脱敏 route report。它不能单独满足 `player.full_playable`。

`player.full_playable` 需要同一次自动化运行产出四个 report：

| Schema | 作用 | 允许记录 |
| --- | --- | --- |
| `astra.player_automation_script.v1` | 描述启动、点击、按键、等待、截图采样、音频采样和期望 route/system UI 状态 | target/profile/platform、公开 scenario 相对路径、target region id、按键名、等待条件和期望 check id |
| `astra.player_input_transcript.v1` | 记录每个真实平台输入事件和 player event loop 接收情况 | event source、坐标、按键、target region、frame hash before/after、focus state、event-loop receipt、diagnostic |
| `astra.player_automation_report.v1` | 聚合 input transcript、visual report、audio report 和 route report | check id/status、region hash、meter summary、host evidence id、route coverage 和 blocking diagnostic |
| `astra.player_presentation_report.v1` | 证明 renderer-ready glyph command 经 Player command sink 到真实平台 GPU | package/profile/build/session、renderer/font provider、layout/command/capture hash、frame sequence、尺寸和变化像素 |

Transcript identity 必须由完整 canonical JSON 计算；序列化失败或 audio meter 出现 NaN/Infinity 时，`player.transcript_serialization` 以 `ASTRA_PLAYER_TRANSCRIPT_SERIALIZATION` 阻断，不能对空 bytes、被 JSON 静默替换的值或部分字段计算成功 hash。

这些 report 不得记录截图、音频、影片、商业正文、商业素材 payload、本地绝对路径、用户名、native handle 或 host secret。视觉证据只能写 region id、尺寸、frame hash 和 changed/blank 状态；音频证据只能写 bus id、peak/rms 区间、sample count、host provider evidence 和 silent/blocking 状态。

## Shared Player Core

当前已新增 `astra-player-core` 的 automation script/transcript/report schema 与 validator，以及 `astra-player` 的 Windows/Web report 校验入口；它们能阻断 direct `route_scenario`、DOM click、JS callback 和直接 `VnPlayerCommand` 冒充 live input。后续平台 host 实现仍需要加载 `.astrapkg`、运行 `VnRuntime`、维护 `PlayerScene`、处理 `PlayerPlatformEvent`，并输出 route/visual/audio/input report。live-player gate 只能调用 `apply_platform_event(event: PlayerPlatformEvent)` 一类平台事件入口，不能暴露测试专用 `advance()`、`choose()`、`open_system()` 或直接 `VnPlayerCommand` 快捷通道。

平台事件进入 core 后必须走与真实 player 相同的 event loop：

1. platform driver 发现并聚焦 player host。
2. driver 注入 mouse/keyboard/touch input。
3. host event loop 把输入转换为 `PlayerPlatformEvent`。
4. `astra-player-core` 推进 runtime、presentation、audio graph 和 route state；`PlayerHostCommand::PresentScene` 只转发 renderer-ready glyph/texture/sprite/rect command，不允许 Player CPU raster 或 headless frame 代替平台输出。
5. 同一次 run 采样 frame/audio/route/input evidence。

`VnPlayerCommand` 只允许留在产品内部语义层和明确的局部单元断言，不得作为 Headless、live-player gate 或产品自动化输入。旧 YAML Headless runner 已删除。

## 可选 Headless 自动比较

Migration 11 已实现平台无关 `astra.user_input_sequence.v1`。它只包含 focus/resume、keyboard、IME、pointer、wheel、touch、gamepad、固定时间推进、await、checkpoint 和 shutdown；`advance`、`choose`、`open_system`、直接 `VnPlayerCommand` 和 Runtime mutation 在 schema/adapter 边界阻断。file 与 stdio 现在执行同一物理输入序列并产生等价 manifest/report，损坏 JSONL 和断流会提交 blocked report 后非零退出。

Headless 自动比较与真实平台操作各自独立执行，不再使用审批或 preflight link 把两者串成强制前置链。自动失败仍明确拒绝；PNG/WAV、frame/checkpoint 和 schema/hash 校验保留，不能冒充真实窗口或设备声音。

## Windows Driver

Windows driver 启动真实 Player 窗口，发现并聚焦目标。已有自动化使用 Win32 `SendInput`，并检查 event loop 接收结果及 dialogue、choice、system page、config、save/load 和 backlog 的实际变化。

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

已保留的 Web driver 使用本地 HTTP bundle、真实浏览器页面与 CDP 输入；不能通过 DOM click、直接 app callback 或写 route state 冒充平台事件。Web 已延后，本节是现有工具事实，不是本轮新增产品验收要求。

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
- `astra.player_presentation_report.v1` 缺失、来自 headless、没有变化像素，或 package/profile/build/session/renderer identity 与 capability、conformance、automation 不连续。
- required voice/BGM/SE 期望存在但 AudioGraph、WebAudio 或 WASAPI meter 静音。
- 缺 Windows/Web host evidence。
- 发现 `VnPlayerCommand`、`--route-scenario` 自推进、`--dump-dom` route runner、DOM `element.click()`、JS callback 或直接 runtime command path。

`input.browser`、`input.gamepad`、`input.touch`、`input.ime` 这类 API 可用性只能说明 capability 存在，不能作为可玩证据。

## 平台范围

本轮验证 Windows/Linux/macOS/Android 的实际输入、媒体、存读档与退出；桌面检查真实窗口和 focus，移动检查 touch、safe area、resume 与设备 audio session。尚未测的平台保持未验收，不把 capability smoke 写成完成；Web/iOS 延后，状态统一见 [实施状态](../status/implementation-plan.md)。
