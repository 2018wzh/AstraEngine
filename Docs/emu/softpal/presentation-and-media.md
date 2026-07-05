# Presentation And Media

SoftPAL presentation 由 script extcall 驱动。旧引擎会直接调用 PAL sprite、audio 和 movie API；AstraEMU SoftPAL core 必须把这些动作转成中立命令，再由 Manager 和 provider 执行。

## Logical scene

`sena-rs` 的 `FrameScene` 默认 logical size 是 `1280x720`。`config.dat`、`system.dat` 和 `system.ini` 可以改启动尺寸或逻辑尺寸。AstraEMU Probe 要记录这些配置是否存在，Load 后由 core 把窗口和 logical size 作为 family state 输出。

Presentation 输出示例：

```text
PresentationCommand::SetLogicalSize { width: 1280, height: 720 }
PresentationCommand::SpriteSet { slot: 12, resource_hash: "...", x: 0, y: 0, z: 0 }
PresentationCommand::TextWindow { visible: true, body_text_id: 0x1234, name_text_id: 0x5678 }
```

`resource_hash` 是 report/debug 字段；正常 render path 应通过 resource id/reference 取 bytes，不把 payload 写进 IPC log。

## Image resources

本地样本大量使用 `.PGD`，少量 `.TGA` 和 `.BMP` mask。`sena-rs::image` 支持：

| signature | 用途 |
| --- | --- |
| `GE ` | PGD/GE base image |
| `PGD3` | delta image，需要 base-resource resolver |
| TGA header | uncompressed/RLE TGA，8/24/32 bpp |

`PGD3` 会读取 base name，把 delta patch 应用到 base image。AstraEMU core 需要把 resolver error 变成可诊断错误，例如 base missing、unsupported bpp、patch 越界。不要把失败时的 image bytes dump 到 report。

## Sprite state

SoftPAL sprite extcall 覆盖：

- load/set image 到 slot；
- set position、alpha、scale、rotation、rect、priority lane；
- transition/backbuffer/copy image；
- face sprite 和 sprite text；
- button sprite 复用。

AstraEMU core 保存的是 script slot 到 internal sprite state 的映射。Manager 只看到 `PresentationCommand`，不看到旧 engine pointer。

## Text and backlog

Text extcall 使用 `TEXT.DAT` id 和 `FILE.DAT` resource id 组合出 ADV text window：

- `text` / `text_w` / `text_a` 更新 body、speaker、voice id。
- `text_w` 启动 reveal，通常后接 `wait_click`。
- `text_set_base` 从 `FILE.DAT` 选择文本框底图。
- history/backlog extcall 维护可回看记录。

Core 可以捕获 `TextCaptureEvent`，但 report 里默认只写 text id、speaker id、hash 和长度，不写完整商业文本。

## Audio

本地样本中：

- `bgm.pac` 是 30 个 OGG。
- `se.pac` 是 449 个 OGG。
- `voice.pac` 是 11,898 个 OGG。
- `system.pac` 也包含系统音 OGG。

`sena-rs` 用 `kira` 加载 static sound。SoftPAL 有 7 个 sound group，slot 数分别是 2、16、8、2、16、64、16；raw handle prefix 分别映射到 `0x10000000`、`0x30000000`、`0x70000000`、`0x20000000`、`0x40000000`、`0x50000000`、`0x60000000`。

AstraEMU 输出：

```text
AudioCommand::Load { group: "bgm", slot: 0, resource: "BGM01" }
AudioCommand::Play { group: "bgm", slot: 0, looped: true }
AudioCommand::SetVolume { group: "voice", raw: 8000 }
```

`PalVolume` raw 范围是 `0..10000`，0 映射静音。provider 可用任意 backend，但 deterministic state 只保存 group、slot、resource、loop、volume 和 position 语义，不保存 native handle。

## Movie

本地样本有 loose `movie/opening.mpg` 以及 localized `movie_cn`、`movie_tc`。`em.pac` 还包含少量 `.MPG`。`sena-rs` 另有 `na_wmv_player` crate 处理 ASF/WMV2/WMA，这是可参考的 decoder 实验，不是 SoftPAL core 对特定 DLL 的依赖。

AstraEMU policy：

- 优先平台 media provider。
- provider 失败时可以使用项目认可的 FFmpeg fallback。
- Core 只发 `MoviePlay/MovieWait/MovieStop` 命令和 hash。
- 不加载 legacy `dll` 目录下的 Windows DLL。

## Render verification

Release Gate 至少覆盖：

- title/menu sprite 是否出现；
- ADV text window show/hide/reveal；
- BGM play/stop/volume；
- SE play/wait；
- voice play/voice wait；
- movie play/wait；
- save/load 后 text/history/memory 是否恢复；
- screenshot hash 只作为定位证据，不提交未授权截图。
