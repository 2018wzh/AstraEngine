# Media Contract

Media 分为 Renderer2D、TextLayout、DecodeProvider、FilterGraph、AudioGraph 和 Timeline。它们执行表现请求，不拥有剧情状态。

## Renderer2D

wgpu 是默认 provider，但不是唯一后端。Renderer2D provider 声明 backend、surface capability、headless support、shader model、target format 和 packaged eligibility。

现有 `HeadlessRenderer` 只证明轻量 CPU contract 和 deterministic frame。Migration 11 planned 完整 Headless Platform 必须显式绑定 Media 层的真实 renderer、font/TextLayout、FilterGraph、AudioGraph 和 decode provider，输出真实 PNG/WAV。state hash 颜色块、矩形变化、空音频、静态 meter 或 synthetic decode 都不能作为完整 Headless 产品证据。

## DecodeProvider

平台解码优先：AVFoundation、MediaCodec、WebCodecs、Windows Media Foundation 等平台模块先接管可用格式。桌面 fallback 通过 optional `ffmpeg-vcpkg` feature 接入，使用 `ffmpeg-next`/`ffmpeg-sys-next` 的 `vcpkg` crate provider 查找 FFmpeg；默认 build 不要求本机 FFmpeg。DecodeProvider 输出 CPU buffer 或 `MediaSurfaceToken`；public API 不暴露平台 native handle。

Player 从 package 消费 encoded audio 时，必须先通过 `asset.catalog` 与 `asset.vfs_manifest` 得到唯一 package-backed entry，执行 bounded read 和 SHA-256 校验，再按文件签名识别 codec。不能用 asset id、文件名或 provider descriptor 猜测已解码成功。Windows Media Foundation 当前返回 `pcm_s16le:<sample_rate>:<channels>`；Player 必须检查格式字段、采样率、声道、sample budget、sample 截断和 frame alignment，再显式转换为 interleaved `f32`。未知格式、空/越界 stream shape 和不完整 frame 都是 blocking，不能转为空音频成功。

Player 的一次性音频必须执行完整资源事务：`OpenDecode -> Decode -> CloseDecode -> OpenAudio -> ordered SubmitAudio -> DrainAudio -> CloseAudio`。每一步都要校验返回的 logical resource；decoded bytes 必须重新计算 hash；drain 返回的 sample count 必须与提交量一致，meter 必须是有限且不高于 `0 dBFS` 的同 run 结果。open 后任一步失败都必须尝试 close，并把 cleanup failure 附加到原始 diagnostic，不能用 `Unit`、空 meter 或静态 report 冒充播放成功。`voice_end` 只能由这条事务完成后产生。持续 BGM、loop、fade、pause/resume 和多 voice mixing 必须由持久 mixer voice owner 实现；在该 owner 接入前，shipping Player 对这些命令返回 `ASTRA_PLAYER_AUDIO_PERSISTENT_UNSUPPORTED`，不得降级为一次性播放。

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

Headless reference output 使用固定采样率、固定声道布局的 PCM S16LE WAV，并保留完整 sample sequence。音频限额、写入失败、静音、削波、声道和时长不匹配都进入 machine-readable diagnostic；不能只记录 peak/RMS 后丢弃实际音频。

## TextLayout

默认 TextLayout provider 使用 `cosmic-text`/Swash。CJK、ruby/furigana、inline wait、voice replay metadata 和 backlog shaping 都必须进入 TextLayout contract，不允许散落到 VN UI 特例。

## Command Boundary

Runtime 只发 `PresentationCommand`、`AudioCommand`、`TimelineCommand` 和 graph refs。Media provider 只能回传 capability、AwaitResult、diagnostic、capture hash 和 profiling evidence。具体 trait、默认 provider 和 graph validation 见 [Asset And Media Pipeline Blueprint](../implementation/asset-media-pipeline.md)。
