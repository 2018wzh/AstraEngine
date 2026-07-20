# TsuiNoSora Classic 的 Director 忠实还原设计

## 1. 设计原则

Classic 还原原作可观察行为，Modern 继续是默认 profile。两者共享同一剧情 Core、route flag、backlog、read-state 与 save payload；差异只能来自 profile policy、UI binding 和 presentation。Classic 系统窗口在 800×600 Yakui 画布内模拟 Director modal，不创建第二个 OS window。

本文只记录稳定 movie/resource id、handler identity、frame/channel、几何、hash 与语义摘要。商业正文和脚本 body 留在私有转换 IR。

## 2. Director movie 映射

当前严格 source reader 已闭合 10 个 movie、2,848 个 frame、428 个 label、1,276 个 frame-action binding。Classic 使用其中 4 个系统 movie和 5 个剧情 movie：

| Director movie | Score resource | frame | label | AstraVN owner |
| --- | --- | ---: | ---: | --- |
| `MENU` | `VWSC-502` | 102 | 8 | Title、隐藏测试菜单、开始/读取/退出 |
| `POPUP` | `VWSC-116` | 39 | 4 | 剧情内系统 modal 与返回原 wait |
| `SAVE` | `VWSC-196` | 9 | 1 | authored save，成功后返回 MENU |
| `LOAD` | `VWSC-160` | 9 | 1 | Title/POPUP 的读取页 |
| `K` | `VWSC-1509` | 594 | 92 | 剧情 state/scene/choice/presentation |
| `S` | `VWSC-1642` | 193 | 23 | 剧情 state/scene/choice/presentation |
| `T` | `VWSC-2724` | 560 | 86 | 剧情 state/scene/choice/presentation |
| `Y` | `VWSC-2017` | 1,074 | 176 | 剧情 state/scene/choice/presentation |
| `Z` | `VWSC-1655` | 240 | 35 | 剧情 state/scene/choice/presentation |

`GLOBALS` 是外部 cast/handler 语义源，不是独立 Score movie。它提供文字速度、音频开关、全局 flag 与公共 handler。每次引用仍通过 `Lscr` resource id、payload hash 与 source-map 证明，不能按 handler 名直接注入 Player。

## 3. UI 与 handler 行为对应

| 原版行为 | 已证明的 handler identity | AstraVN 行为 |
| --- | --- | --- |
| Title idle/start/load/quit | `stay`、`GoToGo`、`PopupLoad`、`quit` | Title page + typed action |
| 隐藏测试入口 | `go[test]`、`go[clear]`、`ready`、`ForTest` | manifest-declared custom system action |
| POPUP close/save/load/close-and-quit | `btnClose`、`btnSave`、`btnLoad`、`btnClose&Quit` | SwitchSystemPage 或 ReturnSystem |
| authored SAVE | `btnSave&G0[MENU]`、`staySave` | host commit 成功后 ReturnSystem；下一 state 进入 MENU |
| LOAD | `btnLoad`、`stayLoad` | validated load request；失败保留页面 |
| 剧情内 save open/stay | `EDOpenSave`、`EDStay` | system frame 保存原 cursor/wait/choice |

这些名字只用于证据索引；实际 effect 由 resource/hash 绑定的 typed program 产生。

## 4. Score channel 与合成层

| channel | 语义 | Classic composition |
| ---: | --- | --- |
| 1 | sky/frame base | 800×600，最底层 |
| 2 | eye/sky overlay | 按 Score 几何与 blend |
| 3 | stage background | 752×424，中心约 `(400,252)` |
| 5 | character | 800×600 frame space，不裁剪到 stage |
| 7 | event/CG | 由 metadata 决定 stage/full-frame clip |
| 9 | shade | 800×600；`mono` 使用 `blend=70` |
| 12 | dialogue frame | 754×82，中心约 `(400,524)` |

人物越过舞台下沿是原作 layer contract，不是截图误差。角色 layer 必须先按 800×600 frame space 合成，再由 dialogue frame 和文字按 Score z-order 覆盖；不能把人物永久裁到 752×424 stage。

两项私有全量源 witness 固化了这个约束：单人物使用 `tsui.asset.0aeba17c8bedd738fc321184`（Director `BITD-487`，command `014983/014985`），双人物使用 `tsui.asset.0f6d609f4c8f8a1df1dcd464`（Director `BITD-1826`，command `015280/015282`）。二者都由 802×602 白色 matte 图严格派生为 800×600 alpha 资产；派生前后 hash、边缘 matte 判定、可见 bbox 与命令绑定写入私有 report。754×82 dialogue frame 也按已验证的白色 matte/透明度比例恢复半透明，不由 UI 主题伪造。

## 5. 阅读与演出 surface

- `classic.opening.staggered`：纯黑全屏，短行按 Director 几何错落排布。
- `classic.opening.centered`：纯黑全屏，窄列居中。
- `talk`：天空、752×424 stage、754×82 dialogue frame和 speaker 区域。
- `mono/monoreturn`：相同 `tsui.surface.monologue`；stage 存在时绘制 70% shade 与居中文字，权威 scene 为黑场时不绘制天空、金线或对白条。
- choice：`GLOBALS` selector 在显示前调用 `tshadehalf`，随后把 STXT 中的全角缩进和 `◆` 当作排版前缀。转换器必须严格验证并剥离这个前缀，再由 Classic view 在 Score 位置绘制唯一一列白色菱形；正文不携带第二枚菱形。页面不使用纸面/金色 modal，也不显示现代 focus ring。

Opening 绑定不靠正文猜测。`T` 的 `director.t.0024` 第 1 个 reading group 绑定 `tsui.surface.opening.staggered`，`Z` 的 `director.z.0111` 第 23 个 reading group 绑定 `tsui.surface.opening.centered`；二者仍引用 private localization key。其他 `mono/monoreturn` 继续使用普通舞台叙述 surface。绑定由 movie、frame node、reading group、reading mode 和 source hash 共同校验。

speaker 与正文只从已证明为 CP932 的 cast/STXT 字段严格解码；非法 byte sequence、未终止字符串或边界不明时阻断。

## 6. Classic system policy

Classic 只声明 `slot.01` 至 `slot.08`，拒绝 `slot.09` 和 quick slot。允许页面为 Title、POPUP、Save、Load/restore、Config、Exit 和 manifest 声明的隐藏测试页。ReadingMode 只包含 Hidden、Manual、FastForward；audio 只通过 typed BGM/SE enabled state 修改。

Save/Load 严格显示 8 行文本槽位，不显示缩略图、游玩时间或 Modern metadata。底层 save v4 仍保存这些信息。Save、Load、Config tab 使用 `SwitchSystemPage` 原位替换 top system frame，不增加 stack depth。authored SAVE 只有在 host persistence 与 metadata commit 成功后返回；失败留在当前页面并返回根因 diagnostic。POPUP 关闭恢复原 story cursor/wait/choice。

系统页的底层画面不是 Host 临时截图。`SystemPageViewModel` 从序列化 `return_cursor`、`return_wait` 和 `return_choice` 重建 typed underlay：Title 下重绘同一 system story entry，剧情内重绘当前 message/choice surface。入口不唯一、backlog identity 缺失或 return state 无法解析时直接阻断。这样 Title Load 保留绿色标题与三个按钮，POPUP/Save/Load/Config 保留进入系统页前的 stage、人物和对白，同时 save/load/replay 后仍可确定性重建。

## 7. 隐藏测试菜单

Title 左上 `64×64` hotspot 是唯一入口，不增加可见按钮。`ForTest` 证明的 `global.mode` 与 `global.panty` 四种 mutation，以及 MENU cast member `82..89`、`92..98`、`102..105`、`112..118` 对应的 26 个 movie/label jump，均编译为 manifest-declared `SystemActionProgram`。UI 只提交稳定 action id；Runtime 再次校验 action、mutation scope/key 和目标 state。跳转关闭 system frame，并写入 route flag、mutation trace、save/replay 和 state hash。未声明 action、任意变量名或任意 jump target 都会阻断。

## 8. GPU E2 checkpoint

Classic 完整 checkpoint 为 Title、Title Load、两种 Opening、背景、普通对白、单人物越框、双人物越框、monologue、choice、POPUP、Save、Load/restore、Config、Exit、隐藏测试菜单。

2026-07-19 的 v43 比较复用同一份通过 18 个序列化物理输入 checkpoint 的 GPU capture identity，report 显式绑定 `wgpu_offscreen`、Windows DX12 hardware adapter、scene submission、raster identity 和 text-first locator evidence。15 项归一化原版参考中有 13 项通过。标准阈值仍是几何误差不超过 2 px、SSIM 不低于 0.94、感知误差不高于 0.08；`TSUI1999-UI-006` 在 anchor、visible bbox、越框、z-order、资源闭包和连续 GPU 帧均已证明的前提下，使用项目所有者具名批准且 hash-bound 的固定 `capture_palette_v1`，SSIM 下限 `0.75`、感知误差上限 `0.12`，几何门禁不变。新增 802×602 连续帧已证明 UI005 是 `Y` frame 26 的稳定黑层状态，也证明 UI009 是 `Y` frame 32 的稳定 shade 状态；两项不再使用 `recapture_required`。这不会把旧 v43 报告升级为 15/15，必须用入口 Score snapshot 修复后的同身份 package 重跑。Choice 的双菱形实现缺陷已经修复，当前菱形列几何误差为 1 px。Windows E3、当前 package 完整路线矩阵和 formal human signoff 继续保持 `IN_PROGRESS`。

2026-07-20 的 private RC 已完成新的像素预检。Y、Title 与 Classic 系统页的 13 项均满足固定比较门禁；模型已查看每项 reference、capture、mask、absolute diff 与 perceptual heatmap。UI010 至 UI014 的系统窗几何偏差为 0 px，UI009 的选择菱形列偏差为 1 px。Y 路线另由 445 条序列化物理输入完整推进到 K movie 首个权威 wait，包内其余 36 条路线不作可玩保证。K movie 的 UI004/UI015 仍留在全量研究报告中，但不阻断本轮 RC。30 张原始捕获已全部通过严格格式检查，12 组 A/B 稳定复帧闭合；UI002 的单帧稳定性由 Score bitmap 资源闭包证明。权威 manifest 与 node map 已同步，当前只等待最终同身份重跑和 formal human signoff。Windows E3 已延期，不属于本轮 RC 门禁。
