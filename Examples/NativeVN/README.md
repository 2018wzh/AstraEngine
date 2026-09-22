# 玻璃雨中的信号 / Signal in the Glass Rain

这是直接替换原两路线技术样例的 NativeVN 旗舰项目。保留原有 180 条双语对白、三条路线与授权视听资产，正在接入当前 typed `VnSession` 产品路径。`.astra`、UI 和 controller 是作者源码；项目刷新工具不再根据旧剧本 JSON 覆盖它们。

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
