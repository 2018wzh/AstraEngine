# Media Contract

Media 分为 Renderer2D、TextLayout、DecodeProvider、FilterGraph、AudioGraph 和 Timeline。它们执行表现请求，不拥有剧情状态。

## Renderer2D

wgpu 是默认 provider，但不是唯一后端。Renderer2D provider 声明 backend、surface capability、headless support、shader model、target format 和 packaged eligibility。

## DecodeProvider

平台解码优先：AVFoundation、MediaCodec、WebCodecs、Windows Media Foundation 等平台模块先接管可用格式。桌面 fallback 可通过 optional FFmpeg feature 接入；默认 build 不要求本机 FFmpeg。DecodeProvider 输出 CPU buffer 或 `MediaSurfaceToken`；public API 不暴露平台 native handle。

## FilterGraph

视觉 FilterGraph 是 typed node graph：

```yaml
schema: astra.filter_graph.v1
targets: [background, character, ui, text, video, final]
nodes:
  - id: bloom_main
    kind: astra.filter.bloom
    input: final
    output: final
    params: { intensity: 0.35, threshold: 0.8 }
```

Node 必须声明 input/output target、参数 schema、GPU/CPU capability、determinism、fallback 和 release gate rule。

AstraEMU filter preset 复用同一 `FilterGraph`。final-frame preset 作用在合成后画面；per-layer preset 绑定 `PresentationCommand` 的 layer id 或 role。family 缺少 layer metadata 时，Manager 只启用 final-frame preset 并输出 diagnostic。

## AudioGraph

AudioGraph 独立于视觉 FilterGraph。它处理 bus、voice、BGM、SE、DSP、ducking、fade、loop、latency 和 platform output。Timeline 负责音画同步。

## TextLayout

默认 TextLayout provider 使用 `cosmic-text`/Swash。CJK、ruby/furigana、inline wait、voice replay metadata 和 backlog shaping 都必须进入 TextLayout contract，不允许散落到 VN UI 特例。

## Command Boundary

Runtime 只发 `PresentationCommand`、`AudioCommand`、`TimelineCommand` 和 graph refs。Media provider 只能回传 capability、AwaitResult、diagnostic、capture hash 和 profiling evidence。具体 trait、默认 provider 和 graph validation 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。
