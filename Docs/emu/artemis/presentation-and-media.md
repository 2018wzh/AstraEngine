# Artemis Presentation and Media

## Stage

`system.ini` 的 `WIDTH` 和 `HEIGHT` 定义虚拟舞台分辨率。Windows 样本分别使用 1920x1080 和 1600x1200；移动和 WASM 段使用 1280x720。`SIDECUT=0` 表示缩放时保留完整舞台并可能出现黑边，`SIDECUT=1` 表示允许裁切。

AstraEMU Manager 负责窗口和实际输出尺寸。Compat core 只输出虚拟坐标、layer state 和 transition fence。

## Layer Tags

官方 graphics tag 首批要覆盖：

| Tag | 行为 | AstraEMU 输出 |
| --- | --- | --- |
| `lyc` | 加载图像到 layer，支持 PNG/JPEG、mask、纯色 layer | `PresentationCommand::LoadLayer` |
| `lyprop` | 设置位置、alpha、anchor、scale、rotate、flip、clip、blend、visibility、drag/click 属性 | `PresentationCommand::SetLayerProps` |
| `lydel` | 删除 layer | `PresentationCommand::DeleteLayer` |
| `trans` | 从当前 layer tree 过渡到未来 layer tree | `PresentationCommand::Transition` + `AwaitToken::PresentationFence` |
| `lytween` / `anime` | tween 或动画 | `PresentationCommand::AnimateLayer` |
| `video` | 全屏视频或视频 layer | `PresentationCommand::Video` 或 `AudioCommand` companion |

`lyprop` 的 `intermediate_render` 只属于 Artemis presentation layer。它不能进入 EngineCore 公共 renderer contract；Manager 只需要知道是否要中间渲染、alpha 和 blend policy。

## Message and Text

scenario tag 涵盖 `font`、`fontinit`、`chgmsg`、`ruby`、`rt`、`rp`、`backlog`、`automode`、`alreadyread`、`skip` 等。AstraEMU 输出两类事件：

| 事件 | 内容 |
| --- | --- |
| `TextCaptureEvent` | 本地结构化文本长度、语言、voice link、ruby span metadata、message layer id |
| `PresentationCommand` | message layer 几何、字体、颜色、glyph、显示/隐藏/tween |

商业剧情正文不进入 report。调试时需要具体文本时，只允许用户在本地 opt-in，并且不写入仓库。

## Audio

| Tag | 行为 | 输出 |
| --- | --- | --- |
| `splay` | 播放 BGM，默认 loop，可设置 gain、pan、fade、buffer | `AudioCommand::PlayBgm` |
| `sstop` / `sfade` | 停止或淡出 BGM | `AudioCommand::Stop/FadeBgm` |
| `seplay` | 播放 SE，需 `id`，可 loop、gain、pan、fade、skippable | `AudioCommand::PlaySe` |
| `sestop` / `sefade` | 停止或淡出指定 SE | `AudioCommand::Stop/FadeSe` |
| `voice` | 与 `seplay` 类似，但为 backlog 记录 replay link | `AudioCommand::PlayVoice` + `TextCaptureEvent.voice_ref` |

`splay` 支持 A-B loop 命名规则，例如 `foo_a.ogg` 进入 `foo_b.ogg` 循环。样本里大量 `.ogg.sli` 文件提供 loop label；AstraEMU 应优先读取 `.sli` 的 sample position，再落到文件名约定。

## Movie

官方 `video` tag 区分全屏视频和视频 layer：

| 模式 | 说明 |
| --- | --- |
| 全屏视频 | Windows 依赖系统解码，官方提到 VMR-7、VMR-9、EVR；样本把 WMV/ASF 放在 loose `movie/` 目录 |
| 视频 layer | 支持 Ogg Theora `.ogv` 和 MJA；软件播放，资源消耗高 |
| WASM video | `file` 是 URL；不能读取本地或 PFS 文件 |

本地观察：

- サクラノ詩10th 的 loose movie 是 `.wmv`，header `30 26 b2 75`。
- 终之空Remake2025 的 loose movie 是 `.dat`，同样是 ASF/WMV header。

AstraEMU media probe 不能只看扩展名；需要看 header magic 和 resolver 来源。

## Images and Tables

样本 `.ipt` 文件是小型 table，例如 UI 区域用 `"x,y,w,h"` 表示按钮 crop。`.tbl` 文件是系统 table，包含 game/UI 配置。Core 只需要把这些表交给 family-private config loader，不把表字段提升为公共 Runtime schema。

## Release Gate

- layer command 顺序可 replay。
- `trans`、`video`、SE wait 都产生可序列化 `AwaitToken`。
- BGM/SE/voice 的 id、loop、gain、pan、fade 能进入 `AudioCommand`。
- movie probe 使用 header magic，不依赖扩展。
- case report 不含图片、音频、视频帧或剧情正文。
