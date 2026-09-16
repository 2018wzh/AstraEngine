# Minori 脚本执行

Minori Family 自持 VM、归档、音频和原生 session。Manager 经 Family API 提交物理输入并接收最终帧、PCM 和诊断；不执行 Minori 私有指令。当前进度与真实游戏阻塞见[实施状态](../../status/implementation-plan.md)。

## 控制流与输入

`chain` 是尾链式脚本切换，没有调用栈或 return。目标限 `minori:/scr/` 下的直接 `.sc` 文件，可带已存在的标签；切换前读取并解析新脚本，成功后清除 local 变量、保留 global 变量。缺文件、非法标签和路径穿越直接失败。

`set`、`setglobal` 接受三操作数赋值和五操作数整数表达式，先查 local 再查 global。`wait` 的整数单位为 10 ms，状态保留原始 tick 与换算后的毫秒。时间、输入和选择等待由 Family session 推进。

`message` 使用保留空字段的 space/tab tokenizer。前四项分别是消息 id、voice identity、speaker、正文首段，后续字段用单个空格连接。短消息沿用原解析器的默认字段。`select` 保存选择焦点，确认键或有效指针点击提交分支；读档恢复同一焦点。文字在 Family 内经共享文字能力生成绘制命令，再由 GPU 合成，正文不进入诊断日志。

## Stage 与绘制

`transition` 保存后续 stage 的模式、资源和时长配置。`stage` 按原生参数结构保存：

- 一至两个冒号分隔的前景资源，`*` 表示空项，保留顺序。
- 可选的两个有符号参考坐标。
- 背景资源及其两个坐标。
- 最多十组立绘文件名和 `position[,resource_parameter]`；省略第二个参数时为 0。

参数列表允许一个末尾空字段，内部空字段、无效整数和不安全资源名拒绝。文件名中的逗号不会被拆成立绘参数。解析完整且校验通过后才替换当前 stage；不再将立绘参数丢弃后转换成 `(0, 0)` 图层，也不通过毫单位转换截断坐标。

`MinoriRuntimeState.stage` 是播放与恢复共用的场景状态。Family 的 `Scene` 使用 SDK `TextureCache` 和公共 `WgpuOffscreenRenderer`，正常帧与读档重建走同一入口。当前已接入单前景资源、背景和整数定位；双资源序列返回 `ASTRA_EMU_MINORI_STAGE_SEQUENCE`，立绘绘制返回 `ASTRA_EMU_MINORI_STAGE_STAND_POSITION`。这两项仍需完成原生绘制语义，不能忽略第二资源或附加参数后声称支持。新的失败帧不覆盖此前有效像素。

已有 `CrossFade2` 时间状态与最后可见帧保存独立于 stage；渲染层叠加实际可见资源和 alpha。未知效果、非法资源及不支持的时间参数拒绝。`.panel 1` 使用消息面板资源，其他形式仍须按实际 handler 继续接入。

## 音频

资源后缀沿用 `resource[volume,pan]` 规则。BGM 和三个 SE bus 由 Family 音频 worker 执行；`*` 是停止请求，保留 fade-out 参数。资源统计与运行时使用同一停止符，不将其列为缺失文件。非循环音频完成、音量与淡出状态由实际混音进度决定。

解码和混音复用 SDK/Engine 能力；Host 消费有界 PCM 队列。关闭先取消阻塞写入，再等待 worker 结束。单元测试和静态资源命中不能替代真实设备播放验收。

## 保存与恢复

VM 数据使用 `astra.emu.minori.runtime_state.v8` 和 postcard，包含脚本身份、PC、变量、等待、完整 stage、transition、效果时间状态、面板和音频等字段。v7 及更早数据直接拒绝，不迁移旧图层表示。stage 的资源角色、名称、序列长度和立绘数量在保存、解码和恢复边界校验。

Family 存档容器另外保存当前显示消息、等待余量和音频快照。恢复先验证游戏及脚本身份，再构造候选 VM、GPU 场景和音频状态；场景重建失败不得将其标为恢复成功。归档解密后的完整素材和 GPU 资源不进入 VM 数据。损坏存档读取失败不得覆盖原文件。

回归覆盖完整 stage 参数往返、非法恢复不修改 live VM、GPU 定位与恢复画面一致、未实现序列拒绝后保留此前像素。真实《夏空的英仙座》的双资源 stage、立绘附加参数、剩余演出和结局仍保持开放。
