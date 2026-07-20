# TsuiNoSora Classic Design QA

## 范围与固定门禁

本页记录 2026 年 7 月 19 日 v43 Classic 同节点视觉复核。GPU capture 来自 `wgpu_offscreen` 的 Windows DX12 hardware adapter；15 张原版参考统一归一化为 800×600。标准剧情画面必须同时满足几何误差不超过 2 px、SSIM 不低于 0.94、感知误差不高于 0.08。`TSUI1999-UI-006` 经项目所有者明确批准使用固定 `capture_palette_v1`：几何仍为 2 px，SSIM 下限为 0.75，感知误差上限为 0.12。该 profile 绑定 `astra.headless_tolerance_approval.v2` 的文件 hash 与 tolerance-set hash，不能用于 source/presentation 冲突或 transition 缺证据的项目。系统页只豁免字体像素和 Yakui 控件皮肤，窗口、tab、槽位、基线与 hit rect 仍执行 2 px 门禁。

每项均生成 reference、capture、mask、absolute diff、perceptual heatmap 和五联图。模型只能记录视觉结论，不能覆盖自动失败或扩大 mask。

## 结果

| ID | 状态 | 视觉结论 |
| --- | --- | --- |
| `001..004` | 通过 | Title、两种 Opening、开场舞台的画布、层级和基线在容差内 |
| `005` | 阻断 | 参考图的黑色 stage 与同 frame 的 Score/handler 背景可见语义冲突；要求原版连续两帧重新取证 |
| `006` | 有界色彩容差通过 | 人物 alpha 边缘反算得到原版 `(0,0)`、移植版 `(0,1)`，anchor、visible bbox、越框和 z-order 对齐；同节点资源闭包与连续 GPU 帧稳定。固定 profile 下 SSIM 为 `0.758432`、感知误差为 `0.102050`，没有改动人物位置、mask 或 UI 几何 |
| `007/008` | 通过 | 双人物越框与 stage monologue 的几何、shade、层级在容差内 |
| `009` | 阻断 | 双菱形缺陷已修复，当前每项恰有一枚 authored marker，菱形列误差为 1 px；原版与移植版 shade 亮度中位比分别为 `0.3491` 与 `0.3600`，原版 transition 状态仍缺少连续帧证据，SSIM 低于固定阈值 |
| `010..014` | 通过 | 灰色系统窗口、tab、8 槽、配置项和按钮的 geometry/style 门禁通过 |
| `015` | 通过 | 公开 gameplay 基准的云框、金线、舞台、对白框和文字基线在容差内 |

当前结果为 13/15 通过，其中一项使用具名、有界的色彩容差。`005/009` 保持 blocking；不得通过硬编码黑场、降低默认 SSIM、扩大 mask 或把色彩容差挪用到结构/转场差异来关闭。两项都需要从 hash 校验后的原版副本，在同一 Director 节点连续捕获两帧后重跑 v3 门禁。

## 本轮修复

- Director choice 的 STXT 项包含固定的全角缩进与 `◆` 排版前缀。转换器现在严格验证并剥离该前缀，由 Classic view 只绘制一列 authored diamond；Modern 也只接收干净的语义正文。
- Choice detector 按行取最左候选并要求所有 authored marker 形成同一列。日文字形中的菱形分量不会误报，旧版第二列重复 marker 仍会阻断。
- v3 node map 增加有界 `reference_validation`。只有已声明原因可以要求重取证；色彩容差只接受 `capture_color_state_unproven`、同节点资源闭包、稳定 GPU 连续帧与 hash-bound human approval。自由文本原因、单帧、修改固定 profile 或把容差用于未批准节点均被拒绝。
- 剧情截图改为 text-first 定位：脱敏正文或选项 hash 先枚举完整 typed IR 候选，再由 Score frame、handler、资源闭包和物理输入路径选定 occurrence；Opening bitmap text 使用 Score channel 与 bitmap hash。v43 报告绑定候选闭包 hash、approval hash 和 tolerance-set hash，禁止只凭文本相同或文件名猜测节点。

## 未关闭门禁

- `TSUI1999-UI-005/009` 原版连续两帧重新取证与自动比较。
- 当前 37 路线 package 的完整 Headless matrix。
- Windows E3 与 formal human signoff。
