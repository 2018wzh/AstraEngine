# 玻璃雨中的信号 / Signal in the Glass Rain

这是直接替换原两路线技术样例的 NativeVN 旗舰项目。默认入口正在重构为连续的分层演出，复用已有授权视听资产；旧故事保留为可玩分支，不要求新演出维持旧对白数量或路线组织。`.astra`、UI 和 controller 是作者源码；项目刷新工具不再根据旧剧本 JSON 覆盖它们。

## 范围

本目录包含以下内容：

- `Narrative/` 与 `Localization/` 下的中英文剧本、路线、镜头、voice cue 和审校记录
- `Visual/` 下的角色、背景、CG、UI、图标、视频、alt text 与生成记录
- `Audio/` 下的原创 BGM、stinger、SE master、分发文件与检测报告
- `Design/` 下的视觉规范、UI tokens、alt-text policy 和代码原生 SVG system icon set
- `Schemas/` 下的四个内容内部 JSON Schema
- `Manifests/` 下的内容索引、provenance 与人工审查记录
- `Scripts/`、`UI/`、`Themes/`、`Controllers/`、`AssetSidecars/` 和 `project.yaml` 组成的 Cook 输入
- `README.md`、`STATUS.md`

当前项目打包 180 条中文配音：林瑶使用 `Eve`、周衡使用 `Rex`，由 `x-ai/grok-voice-tts-1.0` 生成并转为 48 kHz/24-bit WAV master 与 48 kHz OGG distribution。用户已明确授权这些既有声线输出进入发行版。产品验证使用正常 Cook/package/Player 路径，不另建剧情执行器。

## 状态摘要

| 状态项 | 当前值 | 说明 |
| --- | --- | --- |
| `content_creation` | `complete` | 180 条中英文对白、三路线、79 张视觉文件、25 个原创音频源及其分发版本、UI 与视频素材已经入包；不等于可运行或可发布。 |
| `public_release_assets` | `ready_with_authorized_voice` | 180 条配音与其他素材按 manifest/provenance 进入发行 Cook 输入。 |
| `engine_integration` | `cook_ready_with_voice` | `.astra`、项目、UI、localization、283 个 asset sidecar 和 package section 已接入 Cook；未执行 Runtime/Player 验收。 |
| `S3-FLAGSHIP-DEMO-01` | `IN_PROGRESS` | Cook evidence 不能替代 真实平台 Runtime/Player/UI 验证。 |

完整状态见 [STATUS.md](STATUS.md)，迁移边界见 [NativeVN Flagship Demo Migration](../../Docs/migrations/nativevn-flagship-demo-migration.md)。

## Cook

```sh
python Tools/NativeVN/build_nativevn_project.py
cargo run -p astra-cli -- cook Examples/NativeVN/project.yaml --profile advanced-vn --target nativevn-flagship-game --out .tmp/nativevn-flagship-cook
```

该命令只证明 source、UI、localization、asset sidecar、provider binding descriptor 与 package section 能被真实 Cook 主路径接受。Windows/Linux/macOS/Android 输入、画面变化、音频 meter、路线、save/load 和同 run identity 仍留给后续运行验收。

## 新架构接续

项目已迁到 platform host profile v3，明确区分 Kira mixer 与平台 output，并补齐音频缓存/块大小。目标覆盖 Windows、Linux、macOS 与 Android；列入配置不代表设备运行已验证。`advanced-vn` 的系统页绑定与 policy 已对齐，允许存读档、设置和历史页。

当前 target schema 仍要求 `runtime_provider: native_vn` 和 `ui_provider: astra.ui.yakui` 两个元数据字段，因此此处保留；它们不恢复已删除的动态 VN provider。彻底移除须与共享 target/package schema 同步。

## 雨中的片刻

[experience.astra](Scripts/experience.astra) 是默认连续片段：雨夜站台读字、林瑶走近、周衡在前景等待、镜头推进、信号变暗与恢复，再到可保存的重逢。背景、人物、前景、影片、效果与文字有独立深度层；位移、镜头和透明度使用现有 timeline。结束处可听雨看完整影片，或返回选择页进入旧故事。标题页也能重进“雨中的片刻”。

`.astra` 是演出和 UI 的作者源。`Narrative/` 是旧内容批次的素材/配音来源记录，不负责重建当前场景。当前引擎只有镜头缩放，没有人物自身缩放轨道；TaskGroup 新机制尚未集成。没有生成新语音或图片。

演出示例使用现有 timeline 的 fire-and-forget、replace_target、cancel 与阻塞 fence；删除对象时沿用共享取消机制。它没有使用尚未接入的 TaskGroup/Sequence 新语法。保存示例允许在渐变期间打开存档页，并提示退出进程后再读取；实际恢复是否正确仍必须运行验证。新说明文字使用双语本地化，不生成新配音。

可生成连续片段的物理输入，然后交给同树 `astra-headless run --gpu` 执行：

```bash
python Tools/NativeVN/experience_inputs.py --output .tmp/nativevn-reading.jsonl
target/debug/astra-headless validate-input --input .tmp/nativevn-reading.jsonl
```

输入只发送 Resume/Focus、物理 Enter、只读状态等待、checkpoint 和 shutdown，不直接推进剧情或选择答案。该路径检查阅读→并行位移/镜头→替换/取消→可保存时刻，不替代影片、实际写槽/读档、可听音频、完整路线或冷启动恢复。
