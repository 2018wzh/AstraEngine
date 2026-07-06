# Acceptance Matrix

本矩阵定义 TsuiNoSora modernization sample 如何验收引擎完备性。classic profile 验证内容保真和 VN runtime 基线；modern profile 验证系统现代化、修复增强、翻译补丁和 Remake 立绘 overlay。两者都必须通过自动证据和人工 signoff。

## Classic Profile

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

classic profile 的目标是流程与感知等价。像素级和采样级差异记录为诊断，只有内容缺失、流程错误、不可恢复或证据不完整时才阻断。

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
