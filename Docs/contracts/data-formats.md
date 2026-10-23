# Data, Save, Package Contract

Astra 源数据 text-first，运行时数据 binary-first。YAML 适合人写，二进制容器适合发布和加载。

## Source Descriptors

项目、插件、资产 sidecar 和测试 scenario 使用 YAML：

```yaml
schema: astra.project.v1
id: com.example.nativevn
engine_modules:
  renderer2d: astra.renderer.wgpu
  text_layout: astra.text.cosmic
  audio: astra.audio.platform
platforms: [windows, linux, macos, ios, android, web]
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    platforms: [windows, linux, macos, ios, android, web]
    packaged: true
```

每个 YAML schema 必须有 Rust 类型、schema version、migrator 和验证命令。

当前 target 使用 `astra.target_manifest.v3`，删除 gameplay/UI provider 字段；NativeVN 使用 `vn.compiled_project` root v4、compiled project v5 与 blueprint v2。包保留 story、typed UI blueprint/binding/source map/controller/theme 和静态 Yakui backend manifest；动态组件 trust/artifact section 已删除。旧 target/policy schema 与旧 compiled root 直接拒绝，需重新 Cook。

`astra.asset.v1` 的 Rust 真源是 `AssetSidecar`。除 source/type/license/importer/cook/review 外，它包含默认空的 typed `dependencies: Vec<AssetId>`；font asset 还必须提供 `FontAssetMetadata` 的 family、face index、可选 subset 与有序不重叠 Unicode scalar coverage，缺 metadata、空 family、非法 range 或把 font metadata挂到非字体资产都会 blocking。self、duplicate、missing dependency 和 cycle 分别在 sidecar 或 batch graph 边界阻断。Cook 输出 `astra.cook_manifest.v2`，其中 `asset_cook` 使用 `astra.cook_batch_summary.v1`，只记录 content-bound graph hash、artifact/cache-hit/cooked count 和显式 concurrency limit。NativeVN Cook 从 typed font metadata自动生成 profile-bound `astra.font_manifest.v1`，从适用的 localization section 和 `nativevn.default_locale` 生成 `astra.player_locale_config.v1`；不允许项目手写同名 font manifest覆盖权威结果。Package 把同一摘要写入 required `cook.summary` section；`PackageReader` 要求摘要计数与 `astra.cooked_asset.v1` section 数量一致。旧 cook manifest、缺 summary、计数不闭合或 graph identity无法解析时必须重新 cook，不执行隐式迁移或 fallback。

Cook 支持 project-level `package_sections` descriptor，把脱敏 report/manifest 作为 `Raw` section 写入 package；路径只能是项目相对路径，并可按 target/profile 过滤。终之空的 reference、asset analysis、conversion、mount policy 与 Modern profile 仍有真实消费者，不再生成或要求人工 signoff section。资源抽取、Director container/cast/Lingo/source map 与重排报告只用于对应本地工具，保留其 source hash、长度、唯一性和边界诊断，不把这些中间产物全部复制进发布包。具体字段见 [Sidecar Schema](tsuinosora-sidecar-schema.md)。

补充：`tsuinosora.director_lingo_map.v1` 还记录 `Lctx` entry count/table hash；`Lctx` 和 `Lnam` 都不得输出原始 table payload 或 name/context 字符串。`Lctx` 未按 32-bit entry 对齐、`Lnam` 未按 null-terminated table 证明边界时必须 blocking。

补充：`tsuinosora.director_cast_map.v1` 遇到同一 `CASt` resource 被多个 `CAS*` library/slot 绑定时必须 blocking，避免不唯一 cast member 继续进入 `tsuinosora.cast_source_map_report.v1`。

补充：`tsuinosora.cast_source_map_report.v1` 必须阻断 `tsuinosora.cast_map.v1` 或外部 `tsuinosora.director_cast_map.v1` sidecar 中的正文、payload 或 bytecode 字段；报告只允许记录字段路径、hash、id 和 diagnostic。

补充：`tsuinosora.director_cast_member_metadata.v1` 只能作为 `CASt` payload 内显式脱敏 metadata 使用，允许字段限于 kind、route id、command id、anchor、bounds 和 metadata hash；读取结果可以进入 `tsuinosora.director_cast_map.v1` 和 `tsuinosora.cast_source_map_report.v1`，但原始 `CASt` payload 不得进入 report。

补充：`tsuinosora.director_cast_member_metadata.v1` 的 anchor 必须是数值 `x`/`y`，bounds 必须是非负数值 `x`/`y`/`width`/`height`；layout 字段不能验证时必须 blocking。

补充：`tsuinosora.director_cast_member_metadata.v1` 的 `kind: character_atlas` 必须携带 parts；part id、pose、expression、layer 和 fallback 必须是 safe symbol，anchor/crop 必须是数值，mouth/eye state compatibility 必须是 boolean。合规 parts 可以进入 `tsuinosora.director_cast_map.v1` 和 `tsuinosora.cast_source_map_report.v1`。

补充：`tsuinosora.route_graph_report.v1` 和 `tsuinosora.script_source_map_report.v1` 必须阻断同一 `route_id` 的 terminal/choice signature 冲突；否则 NativeVN package input 不允许生成 `.astra` story 或 scenario refs。

补充：同一 route 内的 `choices` 必须唯一；重复 choice id 会阻断 route graph/script source-map report，避免生成重复 `player_input choose` 或 `.astra` option key。

补充：script source-map fallback 只适用于 route graph 缺失；如果 `tsuinosora.route_graph.v1` sidecar 存在但校验失败，`tsuinosora.route_graph_report.v1` 的 diagnostic 必须继续阻断后续验证。

补充：`tsuinosora.script_source_map.v1` 在覆盖 unsupported `Lscr` bytecode 时，route 只能用 `script_resource_id` 和 `script_payload_sha256` 绑定 `tsuinosora.director_lingo_map.v1` 中的 Lscr resource；缺失、未知 resource 或 hash mismatch 必须 blocking，schema 仍禁止脚本文本、bytecode、payload 和本地路径。

补充：`tsuinosora.nativevn_package_input_report.v1` 会对显式 route 输入执行同样的安全校验；不安全 symbol、非 covered route、重复 choice 或冲突 route signature 必须在写 `.astra` story 和 scenario refs 前阻断。

补充：`tsuinosora.local_gate_report.v1` 不把显式 routes 作为商业 route coverage evidence；`local-gate` 必须从 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生 routes 后才允许写 `tsuinosora.nativevn_package_input_report.v1`。

## Binary Container

Save 和 package 共用自描述容器结构：

```text
AstraContainerHeader
SectionTable[]
SectionPayload[]
FooterHash
```

Section payload 默认使用 `postcard` + serde。大型媒体 payload 可以使用 `Raw` 或 `Zstd` section codec；section table 必须记录 codec、hash、stored hash、offset、length、decoded length 和 migration policy。

容器读取校验版本、长度/边界、section 唯一性和内容完整性，损坏输入返回明确错误且保留原文件。postcard 的 struct 字段按位置编码，使用 `skip_serializing_if` 省略字段会破坏读取布局；可选值应通过显式字段或版本化 schema 表达。

容器 ABI 在 [Package And Save](../implementation/package-save.md) 中锁定：little-endian、8 byte alignment、header magic、section table、schema id、codec、hash、optional encryption descriptor、migration policy 和 footer hash。Encryption descriptor 只描述 provider 能力，不提供 DRM 或访问控制绕过方案。

## Save

Engine/VN 存档保存显式运行数据：场景与任务状态、变量、等待关系、剧情位置、系统 UI、媒体及策略状态。公共运行层由 `EngineSession` 持有，VN 状态由 `VnSession` 与对应 component 持有；Player envelope 不复制运行权威。具体 section 和版本以 Rust 类型及 reader 校验为准，不能依据旧文档版本号接受存档。旧格式、损坏输入或恢复失败必须明确报错并保留原槽与当前会话，不保存闭包、协程栈或 native handle。AstraEMU 使用 Family 自有原生存档，不进入 Engine save container。

## Package

Package 必须包含 cooked assets、compiled `.astra` IR、Luau policy bundle、policy lock/source cache、schema registry、provider policy、module fingerprint、target manifest、release report summary、test scenario references 和 platform eligibility。Runtime 不依赖源 YAML 启动。

Package 通过 Asset VFS 暴露为 package-backed mount。`.astrapkg` 仍是控制面和证据面容器；legacy pack、local authorized source 和 overlay source 必须通过 VFS mount descriptor 表达，不能替代 package/save container。

## Standalone Bundle

Standalone bundle 由已 cook/package 的 `.astrapkg` 生成，不从源 YAML 直接拼装。`astra.standalone_bundle_manifest.v2` 记录 target、profile、platform、entrypoint、package hash、scenario refs、artifact role/hash/byte size 和相对文件清单。Windows bundle 显式传入已构建的 Player 与 Crash Reporter；Web 使用已构建的 WASM 与打包器提供的 loader/AudioWorklet。Bundle 不生成另一套 JavaScript scenario/runtime。报告不得记录私有路径或商业 payload；Web 不在本轮产品验收范围。

### 报告与 package 类型归属

报告和数据格式以 Rust 类型为 schema 真源，不另维护带历史 `DONE`/`SPEC_READY`/E1–E4 状态的完整 DTO 矩阵。当前主要边界：

| 数据 | 实际归属与入口 |
| --- | --- |
| 容器、section、scenario binding | `astra-package`；[Package And Save](../implementation/package-save.md) |
| Cook/compiled project、VN manifest 与策略 source cache | `astra-cook` / `astra-vn-package`；只在包内保存运行输入，report 不复制 payload |
| Player 输入、automation、presentation 与 locale | `astra-player-core` / Player/Cook；[实际自动化边界](../implementation/astra-vn-live-player-automation.md) |
| 平台 profile/capability/conformance | `astra-platform`；[Platform Host](../implementation/platform-host.md) |
| 发布检查 | `astra-release`；[发布契约](release-gate.md) |
| 性能预算、采样与 trace identity | [性能契约](performance.md) |
| Headless 输入、JSONL、checkpoint 与 artifact/run report | `astra-headless-protocol` 与测试工具；[操作手册](../manual/operator-guide.md) |
| 终之空脱敏转换 sidecar | [Sidecar Schema](tsuinosora-sidecar-schema.md)，商业源及视听产物仅留 ignored 私有工作区 |

测试 Host 的 profile/schema 不进入 shipping package。Headless 自动比较保留配置 hash、baseline 与 renderer/input 身份，具名审查、容差批准和 preflight/link 格式已退役。局部 route report、静态 meter、fixture 或直接业务命令仍不能冒充真实输入、画面与设备声音。

NativeVN 当前使用 `CompiledVnProject` 与静态产品契约，旧 `vn.compiled_story` 单根 package、动态 VN extension/provider reader 与通用模型 artifact 规划不再列作当前格式。具体版本和 section 集合直接读取 writer/reader，验收状态统一见 [实施状态](../status/implementation-plan.md)。

### VN 数据权威

`astra-vn-core` 定义剧情和系统状态，`astra-vn-policy` 定义策略数据，`astra-vn-package` 负责产品存档和 package。具体 DTO 与 schema 从实现读取，不另维护包含已删除 crate、动态 gameplay provider 和历史 Stage 状态的类型矩阵。

保存与 checkpoint 可以物化完整语义视图；普通 step 不因此全量克隆或编码历史。源码、Graph 和 Timeline 共用 `.astra`，布局仅为作者元数据。策略 source cache 是包内执行输入，报告只输出身份与诊断，不复制脚本 payload。相关行为与限制见 [AstraVN](../modules/astra-vn.md) 和 [Luau 实现](../implementation/luau-policy.md)。

## AstraEMU 元数据与 Manager 本地记录

AstraEMU Manager 的作品级数据分两类：公开仓库 `2018wzh/AstraEMU-metadata` 发布的只读元数据，以及 Manager 本地 SQLite 记录。两者都不进入 package/save/release report，也不携带商业 payload、正文或本地绝对路径。VNDB 是游戏与版本标识的唯一权威源，Bangumi 只用于游玩进度记录。

### AstraEMU-metadata 发布格式

元数据仓库经 GitHub Pages 发布到 `https://2018wzh.github.io/AstraEMU-metadata/v1/`（常量 `DEFAULT_METADATA_BASE_URL`），主仓以 submodule `Emulator/Metadata` 引用同一仓库。Rust 类型（`astra-emu-metadata`）是 schema 真源，`examples/export_schemas.rs` 导出四份 JSON Schema 供数据仓校验：

| 文件 | Rust 类型 | 内容 |
| --- | --- | --- |
| `index.json` | `MetadataIndex`（`astra.emu.metadata.index.v1`） | `generated_at_unix_ms`、`source_commit` 与 `files[]`（相对路径、sha256、size）；路径只允许 `[a-z0-9._/-]`，不含 `..`，单文件 ≤ 8 MiB，合计 ≤ 64 MiB |
| `compatibility.json` | `CompatibilityDatabase`（`astra.emu.compatibility.v3`） | `CompatibilityEntry { vn_id, release_id, emulator, status, notes, emulator_version, manager_version, platform, issue, updated_at_unix_ms }`，`(release_id, emulator)` 唯一，重复为 `ASTRA_EMU_COMPATIBILITY_DUPLICATE` |
| `presets.json` | `PresetDatabase`（`astra.emu.presets.v1`） | `Preset { vn_id, release_id, emulator, summary, configuration, patch }`，`(release_id, emulator)` 唯一 |
| `cores.json` | `CoreCatalog`（`astra.emu.cores.v1`） | `CoreCatalogEntry`：plugin_id、显示名、版本、ABI、平台、GitHub Releases 资产 URL、sha256、size（≤ 512 MiB）、许可与 release tag |
| `patches/` | `astra_emu_patch::list_recipes` | `catalog.json`（每项 `id`、`title`、`script`，以及非空的 VNDB 作品 `vn_ids` 与模拟器 plugin_id `emulators`）与 Luau recipe、`common/` 模块 |

`emulator` 是核心的 plugin_id；`status` 为五级 `perfect`/`completable`/`flawed`/`boot_only`/`unplayable`。兼容性报告经元数据仓库的 issue 表单提交，维护者打上 `compat:accepted` 后由工作流生成条目 PR。

### 同步与本地缓存

同步由统一联网许可（`app_state["network.allowed"]`）控制；未允许时返回 `ASTRA_EMU_NETWORK_PERMISSION_REQUIRED`。`MetadataSyncClient` 只接受 HTTPS、不跟随重定向，带 `If-None-Match` 请求 `index.json`，304 视为未变化；其余文件只下载 sha256 与缓存不同的项，逐个校验 size 与 sha256，不符为 `ASTRA_EMU_METADATA_HASH_MISMATCH`。

Manager 把整个索引写到 `<data_dir>/metadata.staging/`，复用未变化的缓存文件，重新校验全部文件并解析 compatibility、presets、cores 与 `patches/` 后，才把它整体换为 `<data_dir>/metadata/`。任一步失败都丢弃 staging，旧缓存不变；替换失败为 `ASTRA_EMU_METADATA_CACHE_WRITE`。ETag 与同步时间存于 `app_state["metadata.sync"]`（`{ "etag", "synced_at_unix_ms" }`）。

启动时比较 `<data_dir>/metadata/` 与随包 `<exe_dir>/metadata/` 的 `index.generated_at_unix_ms`，前者不早于后者时使用前者；无效目录被忽略。两者都没有时兼容性、推荐配置、核心目录与补丁目录为空，界面显示“未同步”。

### 兼容性匹配

作品先取 VNDB vID（`external_identity(provider='vndb')`）。发行版本 rID 取 `game_release` 中钉住的值；未钉住时，若该 vID 在兼容性库与推荐配置中只出现一个 rID 就用它，出现多个则为未定，界面提示“选择发行版本以查看兼容性”。随后按 `(rID, 当前所选模拟器 plugin_id)` 查找记录；模拟器未定或没有记录时显示为无记录。推荐配置按同一键查找。没有 vID 时不生成兼容性报告链接，界面提示“先关联 VNDB 作品”。

### Manager 本地 SQLite 记录（schema 6）

schema 6 共 13 张表：`family_configuration`、`appearance_settings`、`library_game`、`external_identity`、`metadata_snapshot`、`play_session`、`manager_settings`、`game_settings`、`translation_profile`、`library_root`、`excluded_game`、`game_release`、`app_state`。与元数据相关的是 `external_identity`/`metadata_snapshot`（VNDB 与 Bangumi 关联及快照）、`game_release`（作品钉住的 rID）和 `app_state`（`network.allowed`、`metadata.sync` 等键）。兼容性数据只保存在上述文件缓存中，不写入 SQLite。

游玩统计经 SQL 聚合派生（`SUM(duration_ms)`/`MAX(start_unix_ms)`/`COUNT`），不另存冗余汇总字段。

### 设备输入键名契约（family ABI v5）

自 `astra.emu.family_abi.v5` 起，`LegacyInputEdge.control` 携带规范键名而非语义动作，语义动作词表（`confirm`/`cancel`/`up` 等）已废弃。规范键名由 `astra-emu-family-api` 的 `input_key` 模块定义（`enter`/`escape`/`space`/`arrow_*`/`shift`/`control`/`character:<a-z0-9>`/`function:<1-12>` 等），指针/滚轮 control（`pointer.*`/`wheel`）保持原拼写。输入等待 `LegacyWaitRequest::Input` 由 u64 位掩码改为键名列表 `keys`。

手柄/键盘重映射是 Manager 层通用能力：设备输入经 `InputMapping`（`astra-emu-manager-core`，schemars 导出）翻译为规范键名后入队，family 只负责把键名注入各自引擎。`default_vn_preset()` 提供面向无手柄支持引擎的通用 VN 预设（确认/取消/导航映射到引擎已理解的键盘键）；`WorkSettings.input_mapping` 允许逐游戏覆盖，启动时生效、离开时恢复全局。

## Migration

每个 schema 使用显式 migrator：

```rust
pub trait SchemaMigrator {
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, bytes: &[u8]) -> Result<Vec<u8>, MigrationError>;
}
```

Release Gate 验证 `minimum_supported_version -> current_version` 的迁移链完整。
