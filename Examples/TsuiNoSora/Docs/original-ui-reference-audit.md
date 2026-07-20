# 《终之空》1999 原版 UI 视觉参考审查

## 1. 证据边界

本文记录 Classic profile 的原版视觉参考。2026-07-18 新增的 14 张本机全屏捕获与一张既有 `Game.png` 基准统一为 `TSUI1999-UI-001..015`。图片本体、裁剪图、contact sheet、OCR 和差异图只保存在 ignored 私有工作区；Git 中的 [original-ui-reference-manifest.json](original-ui-reference-manifest.json) 只含尺寸、hash、裁剪参数和稳定语义。

公开网页截图与 Windows 98 实机录像继续用于交叉核验层级和动态行为，但不再占用稳定 reference ID：

- [原版截图合集](https://amaikahlua.hatenablog.com/entry/2020/12/09/183000)
- [Windows 98 实机录像](https://www.youtube.com/watch?v=x3xBNw6o414)

这些参考不能代替合法原版 source、同 checkpoint 自动差异、Windows E3 或 formal human signoff。

## 2. 裁剪与归一化

本机捕获固定为 3839×2399。游戏区域为同一 1400×1050 矩形：

```text
left=1220 top=674 right=2620 bottom=1724
```

`process_original_ui_references.py` 先验证所有 desktop capture 尺寸一致、裁剪矩形未越界，再以 Pillow Lanczos 缩放为 800×600。既有 1403×1053 `Game.png` 使用单独验证的 `(1,1)-(1401,1051)` 边界裁剪后归一化。输入尺寸、数量、裁剪范围或 legacy 尺寸不一致时直接阻断。

## 3. 稳定参考清单

| ID | 语义 | 关键视觉事实 |
| --- | --- | --- |
| `TSUI1999-UI-001` | Title | 绿色云景、右侧三枚黑底图片按钮、左上隐藏热点无可见提示 |
| `TSUI1999-UI-002` | Opening staggered | 纯黑背景、白字短行错落排布 |
| `TSUI1999-UI-003` | Opening centered | 纯黑背景、窄列短文本居中 |
| `TSUI1999-UI-004` | Stage background | 蓝天、云层与中央 eye overlay |
| `TSUI1999-UI-005` | Dialogue background-only | 黑场 stage、底部 754×82 dialogue frame |
| `TSUI1999-UI-006` | Dialogue one character | 人物越过 752×424 stage 下沿；对白框半透明 |
| `TSUI1999-UI-007` | Dialogue two characters | 双人物独立 z-order 与越框关系 |
| `TSUI1999-UI-008` | Stage monologue | stage 整体压暗、居中文字、无对白条 |
| `TSUI1999-UI-009` | Choice | 压暗 stage 上的白色菱形列表，无纸面/金色 modal |
| `TSUI1999-UI-010` | Title Load | 灰色 Load modal 覆盖 Title；绿色背景和原三枚按钮保留 |
| `TSUI1999-UI-011` | POPUP | 剧情内灰色 tabbed modal；底层 stage、人物与对白保留 |
| `TSUI1999-UI-012` | Config | 文字模式与音频开关，不含 Modern 配置项 |
| `TSUI1999-UI-013` | Load | 8 行文本槽位；与 Save 原位 tab 切换 |
| `TSUI1999-UI-014` | Save | 8 行文本槽位、无缩略图；底层 story/dialogue 保留 |
| `TSUI1999-UI-015` | Legacy gameplay | 云框、金线、角饰、752×424 stage、754×82 dialogue 的公开基准 |

精确 raw/crop/output SHA-256 与尺寸以 manifest 为准；本文不复制 hash 表，避免双重真源。

## 4. 区域级视觉约束

### 4.1 舞台与对白

- 画布固定 800×600，不能宽屏重排。
- stage 约 752×424，中心约 `(400,252)`；dialogue frame 约 754×82，中心约 `(400,524)`。
- sky、eye、stage、character、event、shade、dialogue frame 必须是独立层。
- character layer 使用 800×600 frame space，不能永久裁到 stage。
- speaker 与正文 baseline 由严格解码的 cast/STXT 字段和真实 font metrics 决定。

### 4.2 叙述与选择

- Opening 的两种黑场文字布局是不同 authored surface，不能按正文长度推测。
- `mono/monoreturn` 保持同一 reading surface；stage/black 差异来自权威 presentation state。
- stage monologue 使用约 70% shade，隐藏 speaker 和 dialogue frame。
- choice 复用 shade，选项前置白色菱形；Classic 禁止纸面 panel、金边 modal 和现代 focus ring。

### 4.3 系统窗口

- 在 800×600 Yakui 画布内模拟原灰色窗口，不创建第二 OS window。
- Save/Load/Config tab 原位切换，位置和窗口尺寸保持稳定。
- Classic 只显示 8 个文本槽；Modern 的缩略图和 metadata 仍保存在底层 save v4，但不可见。
- 捕获中的 mojibake 属于 locale 环境问题；产品只接受严格 CP932 恢复后的日文。

## 5. Checkpoint 状态

| Checkpoint | 参考 | 状态 |
| --- | --- | --- |
| Title、Title Load | `001/010` | 同一 Classic package 的物理输入 GPU checkpoint 与区域比较通过 |
| 两种 Opening | `002/003` | 同一 Classic package 的物理输入 GPU checkpoint 与区域比较通过 |
| 背景、普通对白 | `004/005/015` | `004/015` 通过；`005` 的黑色 stage 与相同 Director frame 的背景可见语义冲突，必须重新连续捕获两帧 |
| 单/双人物越框 | `006/007` | frame-space/clip 几何均通过；`007` 按默认门禁通过，`006` 由具名 human approval 绑定固定色彩 profile 后通过，未豁免 anchor、visible bbox、越框或 z-order |
| stage monologue、choice | `008/009` | `008` 通过；`009` 已修复双菱形并达到 1 px 几何误差，但原版 shade/transition 状态缺少连续帧证据，SSIM 门禁仍阻断 |
| POPUP、Config、Load、Save | `011/012/013/014` | GPU checkpoint 与 modal geometry/underlay 比较通过 |
| Exit、隐藏测试菜单 | 共享 `011` 的系统窗口视觉语法 | 物理输入 GPU checkpoint 通过；原始截图集中没有独立画面，行为由 Score/Lingo typed effect 证据约束 |

2026-07-19 的 v43 结果只保留为历史证据，其中 UI015 的几何偏差为 0，但整幅画面的色阶不同，SSIM 为 0.723907、感知误差为 0.110397，不能按字体差异豁免。2026-07-20 的 private RC 已用新增连续帧关闭 `005/009` 的 source/presentation 缺口，并重新生成 13 项 Y 范围像素预检；模型已查看全部 reference、capture、mask、absolute diff 与 perceptual heatmap，自动失败没有被覆盖。

本轮 RC 只保证 Y，因此视觉门禁固定为 UI001、UI002、UI003、UI005 至 UI014，共 13 项。UI004/UI015 属于未保证的 K movie，只保留研究状态。13 项像素预检均已通过；UI002 的原版与 capture 整体文字 bbox 完全一致，差异只落在 384 个字形 raster 像素，该项仍使用覆盖率上限 `0.08` 的紧边界文字 mask。`006` 仍是唯一可使用 `capture_palette_v1` 色彩容差的节点。模型已查看全部五联图，自动失败未被覆盖。30 张输入和 12 组 A/B 稳定复帧已闭合，权威 manifest 与 node map 已更新。Windows E3 显式延期；最终同身份重跑和 formal signoff 保持 `IN_PROGRESS`。
