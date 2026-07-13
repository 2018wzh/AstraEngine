# Asset And Media Pipeline Blueprint

Asset pipeline 负责 import、cook、package。Media runtime 负责播放和表现。两者通过 cooked artifact、manifest 和 provider capability 连接，不共享 mutable state。

## Asset Flow

```text
source asset
  -> importer probe
  -> source copy + sidecar + import audit
  -> cook processor
  -> cooked artifact + dependency key
  -> package section
  -> asset.vfs_manifest + asset.catalog
```

## Sidecar Schema

```yaml
schema: astra.asset.v1
id: asset:/characters/hero/main
source: content/characters/hero/main.png
type: image.rgba
license: project-owned
importer: astra.import.image
cook:
  processor: astra.cook.texture2d
  color_space: srgb
  target_profiles: [desktop, mobile, web]
review: accepted
```

Rust type `AssetSidecar` 是 schema 真源。缺少 license、source hash、importer id 或 cook processor 是 blocking diagnostic。

## Media Commands

```rust
pub enum PresentationCommand {
    DrawSprite(SpriteCommand),
    DrawText(TextCommand),
    PlayVideo(VideoCommand),
    ApplyFilter(FilterGraphRef),
}

pub enum AudioCommand {
    PlayVoice(VoiceCommand),
    PlayBgm(BgmCommand),
    PlaySe(SeCommand),
    SetBus(AudioBusCommand),
}
```

Runtime 发 command，Media provider 执行 command。Media provider 不写剧情状态，只回传 capability、AwaitResult、diagnostic 和 capture evidence。

## Default Providers

| Slot | Default | Fallback |
| --- | --- | --- |
| Renderer2D | optional `wgpu` provider | 当前 headless capture provider；Migration 11 planned 完整 CPU reference provider |
| TextLayout | `cosmic-text` + Swash，`astra.text_layout.v2` packaged font database | 缺字体、hash/face/coverage/eligibility、未声明或乱序 fallback、方向、ruby、预算和 glyph resource lifecycle 均 blocking；不读取系统字体 |
| Image Decode | platform image API | Rust image decoder where profile allows |
| Audio Decode | platform decoder | Symphonia fallback; FFmpeg optional |
| Video Decode | AVFoundation/MediaCodec/WebCodecs/WMF | FFmpeg optional |
| Audio Output | platform-owned output provider（Windows 当前为 WASAPI） | reference queue/meter 不作为 shipping evidence；Migration 11 planned 完整 PCM S16LE WAV output |

## Graph Validation

FilterGraph 和 AudioGraph 都是 typed node graph。Node 必须声明 parameter schema、input/output、determinism、fallback、budget 和 release check。

```yaml
schema: astra.filter_graph.v1
nodes:
  - id: bloom_main
    kind: astra.filter.bloom
    input: final
    output: final
    params: { intensity: 0.35, threshold: 0.8 }
```

## Checks

```bash
cargo test -p astra-asset sidecar_schema
cargo test -p astra-cook import_cook
cargo test -p astra-media headless_capture
cargo test -p astra-media decode_provider
astra package validate target/nativevn.astrapkg --profile desktop-release
```

Expected report: stale artifact、provider-ineligible artifact、decode capability gap、graph schema mismatch 都阻断对应 profile。

Migration 11 不改变 Media owner。完整 Headless host 通过显式 binding 组合 renderer、TextLayout、AudioGraph 和 decode provider，输出真实 PNG/WAV；旧 capture hash 与 meter 继续作为局部 contract 证据，不能关闭完整后端。
