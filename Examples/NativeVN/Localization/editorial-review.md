# 《玻璃雨中的信号》编辑审校记录

## 审校结论

简体中文 canonical 与英文 localization 已完成逐条对齐。当前交付包含 `.astra`、`project.yaml`、UI、Cook asset sidecar，以及 180 条用户授权的中文发行配音；不包含 scenario 或 Player 运行证据。结构、ID、路线、预算、逐 cue 音频绑定和 Cook 校验均纳入门禁；平台演出不在本次证据范围内。

## 交付范围

| 资产 | 结论 |
| --- | --- |
| `Narrative/story-bible.md` | 世界观、悬疑机制、主题边界、三终局和年龄分级已锁定 |
| `Narrative/characters.md` | 两名有声角色的动机、语言习惯、表演边界和本地化约束已定义 |
| `Narrative/screenplay.zh-Hans.json` | 中文 canonical，15 个 scene、180 条有声 line、1 个 choice、3 个 option |
| `Localization/screenplay.en.json` | 英文逐条本地化，结构 ID 与中文完全一致 |
| `Narrative/route-graph.json` | 单一分支点，`truth`、`silence`、`signal` 各自唯一终局 |
| `Narrative/scene-shot-list.json` | 15 个 scene 全覆盖，镜头与第三有声角色限制已明确 |
| `Narrative/voice-cues.json` | 180 个稳定 cue，含录音规格、角色声线和逐 scene 表演 profile |

## 实际统计

统计口径：中文有声文本字符计算 `screenplay.zh-Hans.json` 中所有 line `text` 字段的 Unicode 字符，包含对白标点与数字；另列纯汉字数便于复核。选项、标题、元数据和文档文字不计入。时长按 line 的 `estimated_seconds` 求和。

| 指标 | 实际值 | 目标 | 结果 |
| --- | ---: | ---: | --- |
| 中文有声文本字符 | 6,963 | 6,000–7,500 | PASS |
| 其中汉字 | 6,255 | 参考统计 | PASS |
| 稳定 voice cue | 180 | 160–220 | PASS |
| 共通线 | 600 秒（10:00） | 8–10 分钟 | PASS |
| `truth` 分支 | 385 秒（6:25） | 6–8 分钟 | PASS |
| `silence` 分支 | 385 秒（6:25） | 6–8 分钟 | PASS |
| `signal` 分支 | 385 秒（6:25） | 6–8 分钟 | PASS |
| 任一路线单次通关 | 985 秒（16:25） | 15–20 分钟 | PASS |
| 有声角色 | 2 | 林瑶、周衡 | PASS |
| scene | 15 | 全部具名稳定 ID | PASS |
| choice / option | 1 / 3 | 单一分支点、三路线 | PASS |
| terminal | 3 | 每条分支唯一终局 | PASS |

上述时长是剧本预算，包含台词内的自然停顿，不等于录音完成后的实测成片时长。正式录音后应按 `voice-cues.json` 的头尾静音规格重新测量，并在不改动 cue ID 的前提下调整节奏。

## 中文编辑审校

- 周衡保持第一人称观察视角。他的比喻来自雨、玻璃、列车和审计现场，没有连续诗化或替玩家总结路线优劣。
- 林瑶的技术表达具体，情绪通过停顿、条件确认和边界修正出现。她没有替周衡宣布感受，也没有把共同事故解释成命定关系。
- 共通线按“异常出现、身份绑定、旧事故、系统缺口、处置条件、共同选择”推进；每一场都增加新信息，没有用重复争论填充时长。
- 三条路线都明确承担代价：`truth` 有短期暴露窗口，`silence` 失去共同记忆，`signal` 接受受控测试与限时披露责任。文本不把任何路线写成无成本的标准答案。
- 异常信号没有人格化台词。屏幕内容均由林瑶或周衡读出，不产生第三个有声角色。
- 全文未使用第三方作品台词、歌词、角色或专有世界设定；弦港、澄镜网、折光事故和全部对白均为原创内容。
- 已按自然中文原则清理翻译腔、口号式结尾和机械对照。重要判断落在签名、时限、哈希、撤回和公开窗口等具体机制上。

## 英文本地化审校

- 英文版保持与中文相同的事实、情感强度和路线代价，但不逐字复制中文语序。
- `Lin Yao` 的句法偏精确，完整复述条件；`Zhou Heng` 的观察更自然，内心叙述略有呼吸感。两者声线在英文中仍可区分。
- `Gleam Network`、`Refraction Incident`、`Platform Seventeen`、`paired sensory buffer` 等术语在全文保持一致。
- consent、seal、witness、holder digest、escrow、audit chain 等词按语境区分，没有把不同制度动作统一简化成单一词汇。
- 英文行没有新增情节、角色或信息优势。每条英文 line 与中文 line 共用 `line id`、`speaker`、`scene_id`、`route_scope`、`estimated_seconds` 和 `voice_cue_id`。
- 选项独立保存在 `choices` 结构中，未混入任何对白正文。

## 结构与数据验收

使用 Python 标准库解析全部五个 JSON 资产，并执行以下检查：

- line、scene、choice、option、route、terminal、shot 与 voice cue ID 均唯一，且符合小写安全符号格式。
- 中英文 scene、line、choice、option 的 ID 集与顺序完全一致。
- 中英文对应 line 的 `speaker`、`scene_id`、`route_scope`、`estimated_seconds`、`voice_cue_id` 完全一致。
- 所有 line 的 `speaker` 仅为 `lin_yao` 或 `zhou_heng`。
- 180 个 screenplay line 与 180 个 voice cue 一一对应；speaker、scene 与 route metadata 一致。
- `scene-shot-list.json` 覆盖 screenplay 的全部 15 个 scene，没有额外或缺失 scene。
- route graph 的 scene 顺序与 screenplay 一致；三个分支各有且仅有一个 terminal，terminal ID 不复用。
- route graph 与 screenplay 的 choice option 对目标 route 和目标 scene 完全一致。
- 中文字符数、cue 数、共通线、分支线与单次通关时长均在目标区间。

验收结果：PASS。

## 制作交接注意事项

- 当前发行版包含简体中文配音，WAV master 与 OGG distribution 按 speaker/cue 稳定落位；英文文本暂不绑定英语音轨，不得把中文音轨伪报为英语配音。
- 屏幕上的异常信号使用音效或非语言脉冲，不得加入可辨识的第三人声、合成旁白或隐藏对白。
- 选择界面需让三个 option 保持同等视觉权重。不要用色彩、默认焦点或镜头提前暗示“正确”路线。
- `silence` 终局不应通过恢复性彩蛋推翻记忆归零；`truth` 终局不能省略维护拓扑的短期暴露；`signal` 终局必须保留七十二小时披露期限。
- 后续若调整对白，应先改中文 canonical，再同步英文；任何新增、删除或拆分 line 都必须同步更新 voice cue manifest、路线统计与本审校记录。
