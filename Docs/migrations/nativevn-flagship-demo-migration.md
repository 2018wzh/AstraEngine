# NativeVN Flagship Demo Migration

**标题：玻璃雨中的信号 / Signal in the Glass Rain**

**状态：`IN_PROGRESS`**

**Stage gate：`S3-FLAGSHIP-DEMO-01=IN_PROGRESS`**

本迁移现已进入 Cook 接入阶段：内容交付为 `content_creation=complete`，用户授权配音后的发行素材为 `public_release_assets=ready_with_authorized_voice`，引擎接入为 `engine_integration=cook_ready_with_voice`。本轮只验证真实 `astra-cli cook`，不启动 Runtime 或 Player，也不生成平台 E3 结论。

## 项目与内容边界

旗舰内容已直接替换 [`Examples/NativeVN`](../../Examples/NativeVN/README.md) 原两路线技术样例。项目除 Narrative、Localization、Visual、Audio、Design、Schemas 和 Manifests 外，还包含 `.astra` story、Yakui UI source、theme/controller、283 个 asset sidecar 与 `project.yaml`。配套跨平台生成和校验脚本位于 [`Tools/NativeVN`](../../Tools/NativeVN/README.md)。当前里程碑不增加 scenario 或 Player 启动入口。

内容层记录原创剧情和本地化文本，以及下列脱敏资产信息：稳定 id、role、相对路径或 `private://` alias、scene/route/line 关联、SHA-256、byte size、媒体属性、生成来源、license status、release eligibility 和 review diagnostic。绝对路径、secret、未授权参考、免费账户配音、调试截图和废弃中间产物不得进入提交内容。

四个内容内部 schema 是本包的数据边界：

- [`nativevn.flagship_content_manifest.v1`](../../Examples/NativeVN/Schemas/nativevn.flagship_content_manifest.v1.schema.json)
- [`nativevn.flagship_voice_cues.v1`](../../Examples/NativeVN/Schemas/nativevn.flagship_voice_cues.v1.schema.json)
- [`nativevn.flagship_provenance.v1`](../../Examples/NativeVN/Schemas/nativevn.flagship_provenance.v1.schema.json)
- [`nativevn.flagship_review.v1`](../../Examples/NativeVN/Schemas/nativevn.flagship_review.v1.schema.json)

四个 schema 仍只描述内容内部资料；Runtime 和 package 使用现有 AstraVN、target、asset、UI 与 Cook contract，不把内容 schema 提升为 public runtime contract。

## 固定状态

| 状态项 | 值 | 解释 |
| --- | --- | --- |
| `content_creation` | `complete` | 内容创作阶段按主任务口径完成；不能推导出运行或发布资格。 |
| `public_release_assets` | `ready_with_authorized_voice` | 用户明确授权先前确定的 OpenRouter `Eve`/`Rex` 声线输出进入发行版；180 条 cue 均有 release manifest、hash 和可发行标记。 |
| `engine_integration` | `cook_ready_with_voice` | `.astra`、项目、UI、localization、283 个 asset sidecar、180 条配音绑定与 package section 已接入真实 Cook；Runtime/Player 未测试。 |
| `S3-FLAGSHIP-DEMO-01` | `IN_PROGRESS` | 只有正式内容、授权、引擎实现和 Windows/Web E3 全部有证据后，主任务才可推进 gate。 |

## 内容验收口径

内容交付必须能从 manifest 追溯到 provenance 和 review。图像、字体、语音、BGM、SE、视频和 SVG icon 都要有稳定 id、媒体属性、hash、byte size、来源和授权状态。语音采用用户明确授权进入发行版的 OpenRouter 生成结果；模型、声线、请求文本 hash、源响应 hash、master/distribution hash 和逐 cue 绑定均写入 release manifest。原始响应只保留在 ignored 私有目录。

设计审查还要覆盖视觉规范、UI tokens、alt-text policy 和 SVG icon set。设计文件存在只证明设计约束已记录，不证明字体、shaping、renderer、audio、Yakui/UI component 或 Player 已实现。

## Cook 验收与运行触发条件

本轮允许并要求执行：

```sh
python Tools/NativeVN/build_nativevn_project.py
cargo run -p astra-cli -- cook Examples/NativeVN/project.yaml --profile advanced-vn --target nativevn-flagship-game --out .tmp/nativevn-flagship-cook
```

运行验收只有在另行授权后才开始，并必须通过同一 package/build/profile/session 的 Windows/Web 输入、视觉、音频、route 和 host evidence。Cook、内容 schema、静态检查或设计 review 都不能替代 Runtime/Player/UI E3。

## 状态关系

`S3-FLAGSHIP-DEMO-01` 保持 `IN_PROGRESS`。它与 `S3-TSUI-INTERNAL-DEMO-01`、`S3-TSUI-GATE-01` 一起阻断 Stage 3 顶层关闭，但不反向阻断 Migration 6 或 Migration 9 的共享基础。
