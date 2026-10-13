# TsuiNoSora Sidecar Schema Contract

本文档是 TsuiNoSora target 的 sidecar schema 权威规范。AGENTS.md 通过引用路由到本文档，不在规则文件中重复字段级细节。

## Sidecar Schema 总览

| Sidecar | 用途 | 禁止字段 |
| --- | --- | --- |
| `tsuinosora.route_graph.v1` | sanitized route/terminal/choice/coverage | 正文、bytecode、payload、不安全 symbol |
| `tsuinosora.director_lingo_map.v1` | Lingo resource id、entry id、size、hash、`Lnam` entry count、text-extractable 标志、bytecode-reader 需求 | Lingo names、脚本文本、bytecode、`Lctx` payload |
| `tsuinosora.cast_source_map_report.v1` | reader identity/hash、source relative path/hash、line、route/terminal/choice id、coverage、Lscr resource id/payload hash | `text`、`script_text`、`source_text`、`content`、`payload`、`bytecode`、本地路径 |
| `tsuinosora.script_source_map.v1` | 内部 reader 脱敏 sidecar：reader identity/hash、source relative path/hash、line、route/terminal/choice id、coverage、Lscr resource id/payload hash | 同上 |
| `tsuinosora.script_source_map_report.v1` | 同 `script_source_map`，优先保留 reader source-map evidence | 同上 |
| `tsuinosora.director_resource_map.v1` | resource id、member id、route id、terminal id、choice id、reader id/hash/output_contract、source relative path、container entry id、line、hash、diagnostic | 商业脚本文本、素材 payload |
| `tsuinosora.director_cast_map.v1` | cast map 字段 | `text`、`script_text`、`source_text`、`content`、`payload`、`bytecode` |
| `tsuinosora.cast_source_map_report.v1` | 脱敏 report | 同上 |
| `tsuinosora.nativevn_package_input_report.v1` | project/story/package section/scenario ref 的 report-relative path、role、`sha256`、byte size | story 正文、商业素材 payload、本地路径 |
| `tsuinosora.native_asset_rearrange_report.v1` | source/native 相对路径、classification、hash、byte size、coverage、diagnostic | 商业素材 payload |
| `tsuinosora.conversion_report.v1` | 同 rearrange report | 同上 |
| `tsuinosora.visual_reference_report.v1` | hash、尺寸、区域 id、layout metric、diagnostic | 商业视觉参考原图 |

## `director_lingo_map.v1` 约束

- `Lctx` 资源只能输出 entry count 和 table hash，不得输出 context payload。
- `Lctx` payload size 必须按 32-bit entry 对齐；未对齐时 blocking，不能作为 source-map reader 前置证据。
- 内置 `Lnam` preflight 只接受 null-terminated sanitized name table；未终止或无法证明边界的 `Lnam` 必须 blocking，不能输出或猜测 Lingo name。

## `director_cast_map.v1` 约束

- 同一个 `CASt` resource 被多个 `CAS*` library/slot 绑定时必须 blocking，不能静默保留第一条映射。
- `cast_map.v1` 和 `director_cast_map.v1` sidecar 不得包含 `text`、`script_text`、`source_text`、`content`、`payload` 或 `bytecode` 字段；`cast_source_map_report.v1` 遇到这些字段必须 blocking，diagnostic 只能记录字段路径。

## `director_cast_member_metadata.v1`

- `CASt` payload 只有在显式声明此 sidecar 时才能作为脱敏 metadata 读取。
- 允许字段：kind、route id、command id、anchor、bounds、character atlas part/crop/pose/expression/layer/fallback/state compatibility 和 metadata hash。
- anchor 必须是数值 `x`/`y`，bounds 必须是非负数值 `x`/`y`/`width`/`height`；类型不明或负尺寸必须 blocking。
- `kind: character_atlas` 必须携带 parts；每个 part 的 id、pose、expression、layer 和 fallback 必须是 safe symbol，anchor/crop 必须是数值矩形，mouth/eye state compatibility 必须是 boolean。缺 parts 或 part 字段不合规则必须 blocking。
- 这些字段必须继续传入 `cast_source_map_report.v1`，不得输出 cast payload、正文、bytecode 或本地路径。

## 视觉参考约束

- 默认 `Title.png`/`Game.png` 视觉参考必须校验固定尺寸和 hash。
- 缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 必须让 `visual_reference_report.v1` 和 Stage 3 gate blocking。
- report 只能写 hash、尺寸、区域 id、layout metric 和 diagnostic。

## Route Graph 与 Source Map 约束

- `route_graph_report.v1` 和 `script_source_map_report.v1` 中同一 `route_id` 不能映射到多个 terminal/choice signature；冲突时 blocking。
- 同一 route 内的 `choices` 必须唯一；重复 choice id 必须 blocking。
- `stage3-gate` 只能在 route graph 缺失时使用 `script_source_map_report.v1` fallback；存在 route graph sidecar 但检查失败时 fallback 不能绕过 blocking diagnostic。
- `script_source_map_report.v1` 对 unsupported Lingo bytecode 必须逐个 `Lscr` resource 覆盖；部分覆盖时必须 blocking。
- source 指向含 unsupported `Lscr` bytecode 的 `director_lingo_map.json` 时，route 还必须声明匹配的 `script_resource_id` 和 `script_payload_sha256`。
- route `source_hash` 必须等于 source 的 `sha256`，route line 必须在声明 source line_count 内，声明 source hash 必须匹配现有 report-relative source 文件。

## RIFF/RIFX Container 约束

- Director container 的 declared size 必须精确匹配可读文件大小；mismatch 时不得记录 resource/tag coverage，也不得抽取 embedded payload。
- `XFIR` 只允许 verified exact wrapper 中的 RIFF/RIFX payload 进入同一 reader，wrapper size 必须覆盖整个文件。
- opaque、压缩、尾随未验证 bytes 或 source/hash 断裂的 Shockwave 容器必须 blocking，不能退回线性扫描或伪装成 RIFF/RIFX。
- 从 Director cast map 到 `cast_source_map_report.v1` 的派生只能通过 resource id、FourCC、container entry id 和 extracted payload hash 证明，不得靠文件名猜测或写入素材 payload。
- 手写或外部 reader 产出的 cast sidecar 如果声明 `source_hash`，必须匹配 actual extracted source asset，mismatch 必须 blocking。

## Package Section 约束

- `nativevn_package_input_report.v1` 必须重新校验显式传入的 route；不安全 route_id/terminal/choice、非 covered coverage、重复 choice 或冲突 route signature 都必须 blocking。
- `local-gate` 不能把显式传入的 routes 当作商业 route coverage；真实本地 gate 必须从 route graph 或 script source map report 派生 routes。
- `demo-slice --config` 只能作为私有真实数据切片入口，config 中的 root 只作为运行参数读取。通过 demo-slice 生成可玩 project/bundle 只能证明 demo slice 可玩，不能把完整 commercial gate 或 Stage 3 标为 `DONE`。
- package section release gate 必须统一阻断 `text`、`script_text`、`source_text`、`content`、`payload`、`payload_bytes`、`bytecode`、`bytes`、`commercial_text`、`lingo_source`、`raw_payload` 和 `source_payload` 等 payload-like 字段；唯一允许的 `payload` 键是 `redaction.payload: omitted`。
- NativeVN package input 写入 `PackageSections/*.json` 前必须清洗 payload-like 字段。
- `asset_analysis` 即使 `status: pass`，也必须包含至少一条 analyzed asset evidence；空 `assets` 不能作为 Gate 完成证据。
- `conversion_manifest` 即使所有 routes 都是 `covered`，也必须包含至少一条 converted resource evidence；每条 resource 必须包含 source/native 相对路径、classification、source hash、converted hash 和正 byte size，缺字段或 hash 非 `sha256:` 都必须 blocking。

## NativeVN Package Input 到 Asset Mapping

- route graph/source map 中的 sanitized choice id 必须保留到 `.astra` option key 和 scenario `player_input choose`，不能替换成虚构单选。
- 从 `cast_source_map_report.v1` 的 route-bound member 到 `native-assets/` 的映射必须通过 source hash、converted hash、classification 和 route id 生成 `mount_assets`，不能靠文件名猜测。
- Asset analysis 必须先记录脚本引用、container alias、尺寸、透明通道、visible bbox、edge padding、颜色分布、重复 hash、atlas crop/part、reference match 和分类冲突，再允许重排到 `native-assets/`。

## 产品验证边界

人工 signoff section 和具名批准 gate 已退役。真实路线、视听、存读档、输入与 alias/replacement 行为仍须实际验证；结果记录在实施状态，不通过签署文件代替，也不因自动报告通过而写成产品完成。

## Standalone Bundle 约束

- Bundle 从真实 Cook/package 生成并校验相对路径、artifact/section hash 与 target/profile 身份。
- package 的 `scenario.refs.v2` 继续绑定实际输入文件的 path、section、SHA-256 和 byte size，不因删除旧执行命令而去掉内容校验。
- 终之空 NativeVN 工程和 `Automation/*.jsonl` 来自验证后的私有 typed story IR，不由固定 advance/choose/replay 模板推断路线。
- Player 的实际平台输入适配与报告校验保留，不能通过旧 `--route-scenario` 自推进或直接业务命令冒充真实输入。
- 本地 source/mount alias、边界、资源 hash 与 path/payload 泄露仍校验；未接通或未测的 patch direct-read 不通过文档命令声明为完成。
- `nativevn_package_input_report.v1` 只登记实际写出的工程、源码、package section 和物理输入文件，并记录相对路径、role、hash 与 byte size。
