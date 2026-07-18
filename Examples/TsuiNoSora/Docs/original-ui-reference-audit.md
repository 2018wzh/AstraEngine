# 《终之空》1999 原版 UI 补充视觉参考

## 1. 文档边界

本文整理 2026-07-17 从公开网页取得的原版非成人游戏截图，用于 Classic profile 的结构分析和关键帧验收。截图本体属于商业作品素材，只保存在 ignored 私有工作区；Git 仅记录来源、尺寸、hash、稳定编号和不含正文的视觉结论。

这些截图是 `Title.png`、`Game.png` 之外的补充证据，不能替代本地合法原版、同 checkpoint 捕获、自动差异检查或人工 formal signoff，也不能单独证明路线、输入、音频和存读档语义正确。

## 2. 来源与交叉验证

- 主来源：[甘いカクテル「終ノ空 感想」](https://amaikahlua.hatenablog.com/entry/2020/12/09/183000)。该页面保留多张 1999 Windows 版实机截图。
- 交叉来源：[YouTube「[WIN98]終ノ空 Prologue」](https://www.youtube.com/watch?v=x3xBNw6o414)。动态画面确认云层外框、金线舞台、对白条和立绘越界关系并非单张截图的裁切伪影。
- 旧官方产品 URL 当前无法提供可用的历史截图，因此没有把它作为本轮视觉结论的直接证据。

所有下载文件统一位于 `Examples/TsuiNoSora/.local/work/original-ui-references/`。`tsui1999-ui-reference-contact-sheet.png` 只用于本地模型审查，不进入 Git 或 release evidence。

## 3. 统一命名与文件清单

| ID | 私有文件名 | 原始尺寸 | SHA-256 | 原始图像来源 | 视觉分类 |
| --- | --- | --- | --- | --- | --- |
| `TSUI1999-UI-001` | `tsui1999-ui-001-dialogue-character-overflow.png` | 812×610 | `c59ef8ef45eea4bdd6a429fee200ce6fffb23ce9cad744f03638f5a9cb4073a6` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104194416.png) | 有 speaker 的标准对白；人物跨过舞台下沿进入对白区域 |
| `TSUI1999-UI-002` | `tsui1999-ui-002-stage-centered-narration.png` | 810×608 | `ad8416d351349f352644378c478190fb53037480a342000377938ea30670a4b2` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104194841.png) | 保留天空框与金线舞台，舞台压暗并显示居中多行叙述；没有底部对白条 |
| `TSUI1999-UI-003` | `tsui1999-ui-003-dialogue-background-only.png` | 813×612 | `6bc5124ae16a0f02ed702d79542bc19b7a4a507b78528cb4b05d9c71f052b006` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104195019.png) | 无人物的标准对白，用于确认舞台、speaker 铭牌和对白条的独立层级 |
| `TSUI1999-UI-004` | `tsui1999-ui-004-dialogue-character-standard-a.png` | 810×608 | `0eb45e01213b31bdc9f10c6abd997bf1b98199db86acbdfb6f85a5b34f0d0314` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104195325.png) | 日景人物对白；人物下缘与对白条发生覆盖 |
| `TSUI1999-UI-005` | `tsui1999-ui-005-dialogue-character-standard-b.png` | 813×614 | `95c76fcd3e3f9e1481e14b77dd1c2655b5b4e75a32e5f156ea97b9b1f890b453` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104195707.png) | 夜景人物对白；用于检查高对比背景、立绘层级和对白可读性 |
| `TSUI1999-UI-006` | `tsui1999-ui-006-black-centered-monologue.png` | 810×614 | `b032f72575be8b73e3c2016cc3d50755edcb076aa9b4bcfdb2345cfd54e1bfda` | [原图](https://cdn-ak.f.st-hatena.com/images/fotolife/a/amaikahlua/20201104/20201104200055.png) | 纯黑全屏叙述；无天空框、舞台边框、speaker 和对白条 |

统一命名规则如下：

```text
tsui1999-ui-NNN-<semantic-role>.png
```

- `NNN` 是稳定三位编号；新增参考只能追加，不能复用或重新排序。
- `semantic-role` 只描述 UI/构图作用，不写正文、角色名、路线名或来源页面标题。
- 文件内容变化必须产生新的编号；不能在相同文件名下替换 bytes 后继续沿用旧 hash。

## 4. 对 Classic 实现的约束

### 4.1 标准对白不是唯一阅读 surface

`TSUI1999-UI-001/003/004/005` 证明标准对白至少由以下独立层组成：

```text
sky frame
stage background
character layers that may cross the stage bottom edge
gold stage rule and corner ornaments
speaker plaque
white dialogue strip
dialogue text
```

因此不能把人物永久裁剪在 stage viewport 内，也不能把 speaker 铭牌、对白条和天空框烘焙成随场景切换的单张背景。`Scene2D` 必须允许角色层按转换 metadata 选择 `stage-clipped` 或 `frame-overflow` composition。

### 4.2 舞台居中叙述是独立模式

`TSUI1999-UI-002` 要求 Classic 的 `tsui.surface.monologue` 支持
`stage_centered_narration` 视觉状态：

- 天空外框、金线和角饰继续存在；
- 当前舞台画面被压暗，但不能改变权威场景资源；
- 正文在舞台内部居中排版；
- speaker 铭牌和底部对白条必须隐藏；
- advance wait、save/restore 和 backlog identity 仍沿用同一个 story command。

### 4.3 黑场叙述复用叙述 surface

`TSUI1999-UI-006` 不是第三套由 UI 猜测的 surface。源数据审查表明它与舞台居中叙述
同属 `mono` 阅读模式，因此继续绑定 `tsui.surface.monologue`；黑场来自权威
presentation scene state，而不是 UI 根据正文、颜色或截图 hash 切换。该状态要求：

- 输出完整 4:3 黑场，不绘制天空框、舞台边框和对白条；
- 正文使用原作接近中央的窄列排版，而不是普通底部对白布局；
- `talk` 与 `mono`/`monoreturn` 的 surface 选择必须由 typed conversion IR 显式产生；
- 黑场与普通舞台的差异必须由转换后的 presentation state 决定，UI 不新增推测性
  `black_centered_monologue` surface；
- save/load 必须恢复相同 surface kind、文本布局和 presentation state。

因此本轮实现只需要两类权威阅读 surface：

```text
talk                -> tsui.surface.dialogue
mono / monoreturn   -> tsui.surface.monologue
```

`classic.narration.stage_centered` 与 `classic.monologue.black_centered` 仍保留为两个视觉
checkpoint，因为它们验证不同的 presentation state，但不能据此扩张 UI public contract。

## 5. 视觉 checkpoint 与判定

| Checkpoint ID | 参考 | 必查项 | 当前判定 |
| --- | --- | --- | --- |
| `classic.dialogue.background_only` | `TSUI1999-UI-003` | 4:3、云框、金线、speaker 铭牌、对白条、文字基线 | 2026-07-18 已用 Windows DX12 `wgpu_offscreen` 完成同构普通对白 GPU E2 与模型审查；仍需相同剧情 checkpoint 的自动差异证据 |
| `classic.dialogue.character_overflow` | `TSUI1999-UI-001/004/005` | 立绘 crop、anchor、layer、对白区覆盖、夜景可读性 | 未覆盖，blocking |
| `classic.narration.stage_centered` | `TSUI1999-UI-002` | `tsui.surface.monologue`、舞台压暗、居中文本、底部对白隐藏、wait 恢复 | 未覆盖，blocking |
| `classic.monologue.black_centered` | `TSUI1999-UI-006` | 同一 monologue surface、权威黑场 state、窄列文字、无外框、save/restore | 未覆盖，blocking |

只有上述 checkpoint 全部由同一 package/session 的 Headless E2 产物验证，并经模型实际查看截图后，才能声明 Classic 阅读 surface 覆盖完成。Windows E3 和 formal manual signoff 仍需独立完成。

## 6. 隐私与再分发规则

- 不把六张补充截图、contact sheet、OCR 文本、差异图或裁切图加入 Git。
- 不把图片 bytes、正文、角色名、本机绝对路径或下载缓存路径写入 package/report。
- 对外 report 只允许稳定 ID、来源域、hash、尺寸、checkpoint、区域和判定。
- 如果来源失效，保留 hash 作为本地证据身份，但不能把私有缓存改成仓库镜像。
