# Media Runtime 设计

状态：Target Architecture  
定位：Astra 的真实 2D 表现后端，负责 Renderer2D、TextLayout、Audio、Timeline、Animation、UI 和 executable FilterGraph。

## 1. 目标

Media Runtime 将 PresentationCommand 从 DTO 记录提升为真实可执行 backend：

- Runtime 可显示背景、角色、UI、文本、filter output，并播放 voice/music/SFX。
- Headless backend 可在 CI 中验证 render order、text layout、audio command、filter target 和 deterministic hash。
- Renderer2D、TextLayout、Audio 均可通过 EngineModuleSlot 替换。
- Public ABI 不暴露 SDL、GPU handle、audio device handle、font rasterizer private object 或 Editor widget。

非目标：

- 不实现复杂 3D renderer、物理渲染、大型开放世界 streaming renderer。
- 不让 Media 决定剧情状态；Media 只执行 presentation state 和 transient interpolation。

## 2. 分层

```text
PresentationCommand
  -> PresentationExtractor
  -> RenderGraph / AudioGraph / TextLayout requests
  -> Renderer2DProvider / TextLayoutProvider / AudioProvider
  -> FrameCapture / HeadlessHash / Present
```

Provider slots：

```yaml
engine_modules:
  selections:
    astra.renderer2d: astra.renderer2d.default
    astra.text_layout: astra.text.default
    astra.audio: astra.audio.default
```

每个 provider 必须声明：

- backend features
- headless support
- supported cooked formats
- native handle isolation
- packaged eligibility
- diagnostics code range
- hot reload level

## 3. Renderer2D Contract

Renderer2D public request 使用 DTO：

```yaml
frame: 120
render_targets:
  - id: target:/main
    size: [1920, 1080]
    color_space: srgb
layers:
  - id: background
    order: 0
  - id: character
    order: 100
  - id: ui
    order: 200
draws:
  - kind: sprite
    asset: native:/Backgrounds/Room
    layer: background
    transform:
      position: [0, 0]
      scale: [1, 1]
    color: [1, 1, 1, 1]
    clip: null
```

Renderer2D responsibilities：

- texture decode/upload from cooked asset or development source。
- sprite batching by texture/material/layer/order。
- render target management for layer-aware FilterGraph。
- frame capture metadata。
- lost device / resize / DPI recovery。

错误策略：

- missing texture：use configured placeholder, emit blocking release diagnostic。
- unsupported format：cook-time blocking diagnostic。
- device lost：attempt recreate; if failed, fatal runtime diagnostic with crash bundle。
- layer order cycle：validation blocking diagnostic。

## 4. Text And Font

TextLayoutProvider contract：

```yaml
request_id: text:/frame/120/dialogue_001
font_stack:
  - native:/Fonts/NotoSerifJP
  - native:/Fonts/NotoSansFallback
locale: ja-JP
text_key: loc:/opening/alice_001
style:
  size: 32
  line_height: 1.35
  ruby: true
  wrap: rectangle
layout_box:
  x: 160
  y: 760
  w: 1600
  h: 220
```

Text responsibilities：

- font asset load/cook/atlas。
- fallback font chain。
- shaping sufficient for localization target。
- line wrapping、ruby/annotation extension、vertical text extension point。
- glyph cache invalidation on font hot reload。

验收：

- Dialogue text 可在 headless 中输出 glyph run/hash。
- Missing glyph 产生 locale、font、codepoint diagnostics。
- Font atlas 不进入 save；text state 保存 source key、style、typewriter progress。

## 5. Audio

AudioProvider contract：

```yaml
command_id: audio:/frame/120/voice_001
kind: play
asset: native:/Voice/Alice/opening_001
bus: voice
volume: 0.9
pan: 0.0
loop: false
sync:
  actor: actor:/characters/alice
  timeline: native:/Timelines/Opening
```

Audio responsibilities：

- mixer、bus routing：voice/music/sfx/ui/ambient。
- streaming for music/voice。
- fade、ducking、pause/resume。
- deterministic command ordering。
- save/replay of logical audio state, not native device state。

错误策略：

- missing audio：silence placeholder and blocking release diagnostic。
- decode failure：cook-time blocking if source known; runtime error if package corrupted。
- device unavailable：fallback to silent backend with diagnostics when allowed by profile。

## 6. FilterGraph

FilterGraph 必须可执行，不只是记录 DTO：

```yaml
id: native:/Filters/soft_vn
passes:
  - id: bg_blur
    filter: astra.filter.gaussian_blur
    target: background
    params:
      radius: 2
  - id: character_line
    filter: astra.filter.line_enhance
    target: character
    params:
      strength: 0.4
  - id: final_grade
    filter: astra.filter.color_grade
    target: final
```

Targets：

- `background`
- `character`
- `ui`
- `text`
- `final`

Validation：

- filter provider exists。
- target exists。
- pass params match schema。
- no unsupported package/runtime profile。
- headless fallback hash path exists。

## 7. Timeline / Animation / Camera / UI

Timeline source drives presentation events and save-safe state：

```yaml
id: native:/Timelines/Opening
tracks:
  - id: camera.main
    type: camera
    keys:
      - t: 0.0
        value: { position: [0, 0], zoom: 1.0 }
      - t: 2.0
        value: { position: [30, 0], zoom: 1.05 }
  - id: audio.bgm
    type: audio
    events:
      - t: 0.0
        command: play
        asset: native:/Music/opening
```

Rules：

- Timeline lock 通过 Director 进入 runtime arbitration。
- Timeline cursor、active tracks、pending events 进入 save/replay。
- Animation interpolation 可在 presentation update 中 transient 计算，但 key event emission 必须 deterministic。
- UI state 使用 Actor/Component 或 Presentation source，不由 Editor widget 决定。

## 8. Hot Reload

可热重载：

- texture/audio/font source asset。
- text localization table。
- FilterProfile。
- Timeline/animation source if state-compatible。
- shader/filter implementation in development profile。

Rollback：

- validation first。
- prepare new cooked/derived data。
- switch at frame boundary。
- on failure revert previous resource and emit diagnostics。

## 9. Release Gate

Media release gate 检查：

- all referenced media assets exist and are cooked。
- texture/audio/font formats supported by selected provider。
- text fallback covers configured locales or has accepted missing-glyph policy。
- FilterGraph passes executable or have allowed fallback。
- selected providers packaged eligible and ABI compatible。
- headless verification hashes generated for sample scenes。

## 10. 验收

- Current NativeVN package validation records deterministic headless presentation/image metadata evidence; final packaged runtime must still truly display background、character、UI、text and filter output。
- voice/music/SFX 可播放、暂停、恢复、淡入淡出，并在 replay 中保持 logical state 一致。
- Headless backend 可验证 render order、filter target、glyph run、audio command hash。
- Renderer2D/TextLayout/Audio provider 可通过 EngineModuleSlot 替换并通过 release gate。
- Public API 不暴露 SDL/GPU/audio/font native handle。


