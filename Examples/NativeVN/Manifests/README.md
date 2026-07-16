# Manifests

这里保存旗舰内容包的脱敏索引说明。manifest 只描述稳定 id、角色、相对路径或 `private://` alias、scene/route/line 关联、内容 hash、byte size、媒体属性、生成来源、授权状态和 release eligibility。

内容 manifest 不得成为 Runtime package manifest。它可以索引项目 source 与 sidecar，但 `astra-cli` 只读取现有 AstraVN、target、asset、UI 与 Cook contract。没有实际媒体或授权证据时，不写占位 hash，不把设计文档当成素材 evidence；真实来源只通过 `private://` alias 进入内部审查记录。

对应 schema：

- [nativevn.flagship_content_manifest.v1](../Schemas/nativevn.flagship_content_manifest.v1.schema.json)
- [nativevn.flagship_voice_cues.v1](../Schemas/nativevn.flagship_voice_cues.v1.schema.json)
- [nativevn.flagship_provenance.v1](../Schemas/nativevn.flagship_provenance.v1.schema.json)
- [nativevn.flagship_review.v1](../Schemas/nativevn.flagship_review.v1.schema.json)

当前实例：

- `content-manifest.json`：公开树内内容与媒体的稳定索引；
- `provenance.json`：生成方式、hash、媒体属性与许可范围；
- `review.json`：内容、视觉、UI、alt text、license 与发布准备度审查。
- `cook-evidence.json`：真实 Cook 的 project/profile/target/hash、artifact count 和 asset graph 摘要；不记录本地输出路径，也不冒充 Runtime/Player evidence。

当前项目采用 `public_release_assets=ready_with_authorized_voice`。`voice-release.json` 记录 180 条配音的 canonical line binding、master/distribution、hash、媒体属性、OpenRouter model、voice、请求文本 hash 与用户授权状态；`.astra` 为每条 line 绑定唯一 voice command。
