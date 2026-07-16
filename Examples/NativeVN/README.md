# 玻璃雨中的信号 / Signal in the Glass Rain

这是直接替换原两路线技术样例的 NativeVN 旗舰项目。当前里程碑覆盖 `.astra` canonical story、双语本地化、Yakui UI source、theme/controller、完整中文配音、素材 sidecar 和 `astra-cli cook`；按要求不执行 Player 或 Runtime 测试。

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

当前项目打包 180 条中文配音：林瑶使用 `Eve`、周衡使用 `Rex`，由 `x-ai/grok-voice-tts-1.0` 生成并转为 48 kHz/24-bit WAV master 与 48 kHz OGG distribution。用户已明确授权这些既有声线输出进入发行版。项目仍不包含 scenario、Player 启动脚本或任何绕过 provider binding 的运行时入口。

## 状态摘要

| 状态项 | 当前值 | 说明 |
| --- | --- | --- |
| `content_creation` | `complete` | 180 条中英文对白、三路线、79 张视觉文件、25 个原创音频源及其分发版本、UI 与视频素材已经入包；不等于可运行或可发布。 |
| `public_release_assets` | `ready_with_authorized_voice` | 180 条配音与其他素材按 manifest/provenance 进入发行 Cook 输入。 |
| `engine_integration` | `cook_ready_with_voice` | `.astra`、项目、UI、localization、283 个 asset sidecar 和 package section 已接入 Cook；未执行 Runtime/Player 验收。 |
| `S3-FLAGSHIP-DEMO-01` | `IN_PROGRESS` | Cook evidence 不能替代 Windows/Web Runtime/Player/UI E3。 |

完整状态见 [STATUS.md](STATUS.md)，迁移边界见 [NativeVN Flagship Demo Migration](../../Docs/migrations/nativevn-flagship-demo-migration.md)。

## Cook

```sh
python Tools/NativeVN/build_nativevn_project.py
cargo run -p astra-cli -- cook Examples/NativeVN/project.yaml --profile advanced-vn --target nativevn-flagship-game --out .tmp/nativevn-flagship-cook
```

该命令只证明 source、UI、localization、asset sidecar、provider binding descriptor 与 package section 能被真实 Cook 主路径接受。Windows/Web 输入、画面变化、音频 meter、路线、save/load 和同 run identity 仍留给后续运行验收。
