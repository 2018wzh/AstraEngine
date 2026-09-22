# Acceptance Matrix

本矩阵定义 TsuiNoSora modernization sample 如何验收引擎完备性。classic profile 验证内容保真和 VN runtime 基线；modern profile 验证系统现代化、修复增强、翻译补丁和 Remake 立绘 overlay。两者都必须通过自动证据和人工 signoff。

## Classic Profile

`b8a7ba2cb` 新版 Windows 复测：旧槽拒绝点击、正文槽读回继续、最大化、选择中打开系统页再 Escape 返回、手动选择第二分支、新 slot.05 保存及冷启动读回继续通过。槽列表未显示缩略图卡片，卡片裁剪与键盘缩略图视觉检查仍须在 NativeVN 完成。复测发现窗口级联位置遮挡底部及 UTC 时间未标识；对应工作区与本地时间修复已有增量回归，设备复测待完成。

原版首句实际保存、推进和读取后直接返回正文；Classic 原先读档恢复了保存页，已改为完整恢复后执行系统返回。局部回归检查剧情 cursor/等待保留，以及显式快照恢复仍保留系统页；新版真实 Player 已验证正文读取后继续、选择读取后分支执行。原版系统窗口部分文字在测试环境显示方块，当前只确认可辨认的页签、槽位及操作结果，未据此宣称字体对齐。

设置页原先没有当前选项标记，现绑定权威阅读模式和声音开关并呈现持续选中状态。Windows 实测切换文字隐藏/手动及声音开关、移动键盘焦点、切页重开保持当前值；NullAudio 标记持续显示。重 Cook 的包身份变化暴露目录未提前禁用旧包槽的问题，已复用 Runtime 恢复校验在目录禁用异包槽；用户读档候选拒绝保留当前剧情、媒体和任务，并允许继续保存其他槽，53 项 Player-vn 回归通过。新版 Windows 复测待 Sandbox 释放后进行，不迁移或覆盖旧槽。

2026-09-23 新版复测已完成正常新游戏、两处真实选择、系统页、新槽保存/读取、退出重开与键盘恢复后继续；快进由游戏界面开启，在选择处等待用户确认，未直接修改剧情状态。旧槽保留且禁止读写。选择前和屋顶独白各有正常游玩产生的测试槽，供后续逐类对照。原版同一 MENU 已确认绿底黑云，先前蓝白 READY 不能用于标题色差结论；正文第一句的黑背景与正文顺序一致。原版 Escape 会直接退出，Classic 目前正文中不退出、系统页中返回，操作差异已记录，尚未决定调整。可听音频、全部演出分类与完整 UX 对照仍开放。

本轮转换源清点以实际用于 Cook 的 17 个主剧情分片为准。下表的数量是命令数量，不能当成已游玩的片段数量；片段 ID 用于对应 source map，深层片段仍须通过正常游玩存档到达。

| 演出类别 | 转换源实际使用 | 已选代表片段与当前对照 |
| --- | --- | --- |
| 正文与独白 | dialogue 11,606；monologue 5,315 | 开场第一句已与原版同场景对上；屋顶独白有正常测试槽，原版对照待做 |
| 选择 | 19 个 choice 声明，分支由另 183 个 branch 处理 | 首个两项选择及随后地点选择已实际操作；原版布局、时序与分支对照待做 |
| 图层与人物 | 7 层；show/move 各 1,201；clear_layer 1,073；shade 2,700 | 首次人物越框、背景和选择遮罩已在真实 GPU 显示；与原版逐层对照待做 |
| 转场 | type 26 共 1,625；type 9 共 17；type 10 共 10，均 250 ms | `director.y.0020` 与 `director.y.0038`；已在正常流程经过，关键时序逐项对照待做 |
| 文字和图层显隐 | layer_visibility 31；skip_allowed 18 | `director.k.0117` 的图层显隐尚未实玩；系统隐藏文字待原版配对 |
| 动画与等待 | timeline 10；shake 24；wait 209；input_wait 2 | 开场定时片段已实际运行；`director.y.0038` shake 仍需稳定定位与原版对照 |
| 媒体 | bgm 402；se 121；audio 控制 181；本批剧情没有 video 命令 | 当前只验证共享媒体执行与 NullAudio 生命周期，可听声音未通过；不能用没有使用的 movie 声明影片验收 |

目录、图片和商业文字不随清点结果提交。

2026-09-22 本轮按代表流程与演出分类对照验收，不要求完整路线或新的 signoff 产物。Windows Sandbox 的真实 GPU/显式 NullAudio 已完成新游戏至正文、右键系统页、空槽保存、推进后读回及退出重开读取；命名键修复后方向键、Enter、Escape 已复测。最大化保持 4:3 内容与正确点击位置。选择、逐类演出与原版习惯对照仍未完成；切页黑帧与标题颜色差异保持待修。NullAudio 不计作可听声音通过，旧比较容差不自动豁免本轮明显颜色差异。


| Area | Acceptance target | Automatic evidence | Manual evidence | Blocks release |
| --- | --- | --- | --- | --- |
| Source inventory | 原版资源被脱敏登记，未知项可解释 | `tsuinosora.source_inventory.v1`、hash、count、coverage | source boundary review | 本地路径泄露、payload 进入仓库、coverage 缺口无解释 |
| Route and command cursor | 完整路线可从入口推进到结尾 | scenario route report、command cursor hash、choice payload trace | full playthrough signoff | route 断裂、choice 结果错误、cursor 不可恢复 |
| Dialogue and text | 文本顺序、backlog、read-state 与原版体验一致 | text key coverage、dialogue wait hash、backlog event report | 文本抽样复核 | 文本缺失、顺序错误、backlog 不可 replay |
| Visual assets | 背景、CG、立绘出现时机正确 | asset coverage、presentation hash、source map | 画面复核 | 缺图、层级错误、关键 CG 不显示 |
| Audio and voice | BGM、SE、voice、fence 与等待点一致 | AudioGraph report、voice fence hash、duration coverage | 听音复核 | 音频缺失、voice replay 错误、fence 导致流程错位 |
| Movie and wait | movie、wait、skip 和 resume 行为可确定 | movie wait report、save/load from wait hash | 关键影片复核 | movie 不可恢复、wait state 丢失、skip 破坏 route |
| Input rhythm | 左键推进和右键存档的 classic 语义保留 | input scenario、state hash | 实机节奏复核 | 输入映射错误、save 入口不可用 |
| Save/load/replay | 任意 wait state 可保存、读取并 replay | save/load hash、replay report | 抽样复核 | replay 非确定、save 恢复位置错误 |
| Director system UI | MENU/POPUP/SAVE/LOAD/GLOBALS 的页面、8 槽、Config、Exit 和隐藏测试入口保持原行为 | profile v2、system action manifest、system-frame hash、slot/page/action rejection tests | 原版系统窗口并排复核 | 任意 action/slot、嵌套页面栈、底层画面丢失、保存失败仍返回 |
| Classic special surfaces | 两种 Opening、stage monologue、choice 和人物越框按 Score 合成 | 18 个 `wgpu_offscreen` checkpoint、15 项 v3 比较中的 13 项通过、layer/clip snapshot、具名色彩 tolerance approval | `005/009` 必须补原版连续两帧，模型查看全部五联图 | CPU fallback、stage 裁错、shade/choice/modal 几何偏离、reference 取证冲突 |

classic profile 的目标是可观察行为忠实。几何、layer、clip、shade 和系统窗口结构属于阻断约束；捕获颜色与字体 raster 差异只能由具名 comparison policy 与 hash-bound human approval 限定批准。当前 v43 比较复用同一份 18-checkpoint GPU capture identity，15 项归一化参考中 13 项通过；`006` 仅按不可修改的 `capture_palette_v1` 通过，仍执行 2 px 几何门禁。`005/009` 的失败来自 reference/source 或 transition 稳定捕获证据不足，保持 blocking。

2026-07-23 的修复切片已把 Classic/Modern 的设置与消息文本改为按实际可用宽度 shaping、换行和裁剪，并将消息 reveal 固化为可保存的 grapheme 固定时钟状态；这些是局部 E1/E2 证据，不关闭视觉验收。Director 转场链路现从完整 cast 的受控 Lingo 资源严格提取 helper、类型、时长参数和块参数，并将 scene controller 的实际 helper 调用接入 NativeVN lowering。typed descriptor、旧场景快照和进度会随 Player save v5 保存；Player 按 descriptor 合成 type 1/9/10/26，type 26 使用固定 8×8 Director pattern，而不是降级为 crossfade 或随机 dissolve。IDA 已确认原始 PE 含嵌入式 Director Player；其无符号 builtin dispatch 不能单独导出可审计的 type→像素算法，因此 pattern 与时长单位只在静态 Player identity 和兼容实现交叉核对后进入 descriptor，未知或不完整资源仍 fail-closed。此次从私有完整 dump 重新生成 Y 线 Story IR 时，外部 cast 目录的结构与 reader 要求不一致，严格 preflight 已阻断生成；所以尚未产生与本次代码相同 build/package/profile/input identity 的 Y 线 Headless E2。Windows E3 仍是后续人工输入复验，不以本轮静态分析替代。

## Modern Profile

| Area | Acceptance target | Automatic evidence | Manual evidence | Blocks release |
| --- | --- | --- | --- | --- |
| System UI | 标题、存读档、backlog、auto、skip、config、gallery、replay、route chart、voice replay 可用 | system scenario、UI state report、Core hash unchanged | 操作流复核 | system page 改写 Core state、关闭后不能回到 classic |
| Filter profile | 缩放、锐化、色彩、低分辨率修复可回退 | filter preset report、input/output hash、fallback id | 画面复核 | 关键画面裁切错误、fallback 缺失 |
| Audio repair | 降噪、响度均衡、声道修复不破坏时序 | audio preset report、duration/fence hash | 听音复核 | 时长变化影响 fence、voice replay 失真 |
| Chinese patch | 文本覆盖通过 patch package 独立启用 | localization coverage、overflow report、Core hash unchanged | 校对 signoff | 译文来源不可提交、key 冲突、关闭后仍影响 classic |
| Remake portraits | 立绘 overlay 可按角色和场景启用 | alias/replacement report、fallback report | replacement review | 替换错人、裁切异常、fallback 缺失 |
| Package composition | patch、profile、filter、overlay 可独立开关 | package manifest、release report | 发布包抽检 | 商业 payload 混入、profile 间互相污染 |

modern profile 的增强风格限定为修复增强，不做强风格重制。任一增强项必须能关闭，关闭后 classic profile 的 route、save/replay 和 Core state hash 不应变化。

## Engine Completeness

| Engine capability | Demo evidence |
| --- | --- |
| AstraVN command cursor | route scenario、dialogue wait、choice payload、wait/movie/fence |
| EngineCore StateMachine integration | VN step action trigger event、deterministic state hash、rollback scope |
| Asset and media pipeline | image/audio/movie coverage、source map、release report |
| Presentation and Timeline | presentation hash、timeline join/cancel、voice fence |
| Package and save | package manifest、save/load from wait、replay report |
| Luau policy boundary | system UI and presentation effect reports without Core state mutation |
| VFS provider boundary | direct-read report, hash verification, no commercial payload in package |
| Release Gate | joined source/conversion/modern/manual report with blocker summary |

Demo 不能只靠自动 scenario 宣称完成。完整验收必须同时具备 release report 和 `tsuinosora.manual_signoff.v1`，并且人工完整通关、听音、画面和 alias/replacement review 都没有阻断项。


本轮关闭/恢复回归：旧图片预取失败在成功读档换代后被拒绝；停止时未执行队列丢弃，在途任务全部 join；缺失转场源资源的合法容器在 World 提交前拒绝，原场景继续且可再次保存。以上为开发回归，Windows 真实选择与逐类演出对照仍未关闭。


旧槽目录回归已覆盖：拒绝旧头部后仍能启动和推进；该槽在缩略图准备与直接保存入口均禁止覆盖；另一槽保存、读取并继续后，旧槽仍受保护。新版 Windows 实玩确认不可用状态显示、点击旧槽无副作用、Escape 返回、新游戏与未声明快捷槽时 F5/F9 无副作用。进入保存页曾因 Controller 强制聚焦受保护的 slot.01 而退出；现按 can_write 选择首个可写槽，全部不可写时聚焦返回按钮，Classic/Modern 四种组合回归通过。修复后真实保存闭环继续复测。
