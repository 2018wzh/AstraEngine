# Media Contract

Media 分为 Renderer2D、TextLayout、DecodeProvider、FilterGraph、AudioGraph 和 Timeline。它们执行表现请求，不拥有剧情状态。

## Renderer2D

wgpu 是默认 provider，但不是唯一后端。Renderer2D provider 声明 backend、surface capability、headless support、shader model、target format 和 packaged eligibility。

## DecodeProvider

平台解码优先：AVFoundation、MediaCodec、WebCodecs、Windows Media Foundation 等平台模块先接管可用格式。桌面 fallback 可通过 vcpkg 接 FFmpeg。DecodeProvider 输出 CPU buffer 或 `MediaSurfaceToken`；public API 不暴露平台 native handle。

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

## AudioGraph

AudioGraph 独立于视觉 FilterGraph。它处理 bus、voice、BGM、SE、DSP、ducking、fade、loop、latency 和 platform output。Timeline 负责音画同步。

## TextLayout

默认 TextLayout provider 使用 `cosmic-text`/Swash。CJK、ruby/furigana、inline wait、voice replay metadata 和 backlog shaping 都必须进入 TextLayout contract，不允许散落到 VN UI 特例。
