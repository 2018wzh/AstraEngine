# Conversion Manifest

本页定义 TsuiNoSora modernization sample 的脱敏 report schema。字段是 planned data contract，用来约束后续转换器、VFS 插件和 release gate；当前仓库不包含这些 report 的实现。

## Redaction Rules

所有 report 必须遵守：

- 使用 `original_install_root`、`remake_install_root`、`local_work_root` 作为 root alias。
- 不记录本地绝对路径、用户名、环境变量或原始 payload。
- hash 使用不可逆摘要，默认记录算法、摘要值和输入 logical id。
- 文本只记录 key、长度、语言、覆盖状态和校对状态，不记录正文。
- 图像、音频、影片只记录尺寸、时长、编码类别、hash、coverage 和诊断码。

## `tsuinosora.source_inventory.v1`

用于记录原版与 Remake 资源脱敏清单。

```yaml
schema: tsuinosora.source_inventory.v1
source_profile: original_1999
root_alias: original_install_root
inventory_id: original_1999.base
generated_by: astra.sample.tsuinosora.inventory
generated_at_utc: "2026-07-06T00:00:00Z"
summary:
  containers: 0
  resources: 0
  known_resources: 0
  unknown_resources: 0
  coverage_percent: 0.0
resources:
  - logical_id: original_1999.container.ready
    kind: director_movie
    container: root
    size_bytes: 0
    hash:
      algorithm: blake3
      value: redacted-hash
    coverage_status: discovered
    diagnostics: []
```

核心字段：

| Field | Meaning |
| --- | --- |
| `source_profile` | `original_1999` 或 `remake_portrait_overlay` |
| `root_alias` | 只能是约定 root alias |
| `logical_id` | 与本地路径无关的稳定资源 ID |
| `kind` | `director_movie`、`cast_library`、`image`、`audio`、`movie`、`script`、`font`、`unknown` |
| `coverage_status` | `discovered`、`parsed`、`converted`、`manual_review`、`missing`、`ignored_by_policy` |

## `tsuinosora.conversion_report.v1`

用于记录解包、重排、NativeVN 转换和缺失资源。

```yaml
schema: tsuinosora.conversion_report.v1
source_inventory_id: original_1999.base
target_profile: classic
converter_version: astra.sample.tsuinosora.converter.v1
summary:
  route_count: 0
  command_count: 0
  converted_assets: 0
  missing_assets: 0
  manual_review_items: 0
  full_playable_ready: false
routes:
  - route_id: classic.main
    command_count: 0
    choice_count: 0
    wait_state_count: 0
    media_bindings: 0
    coverage_status: manual_review
resources:
  - logical_id: original_1999.cg.opening
    native_id: native.image.opening
    source_hash: redacted-hash
    converted_hash: redacted-hash
    rollback_scope: command
    coverage_status: converted
diagnostics: []
```

必须覆盖：

- command cursor、dialogue wait、choice payload、wait/movie/fence。
- route stack、call frame、system page entry/exit。
- source span、debug symbol、rollback scope。
- resource mapping、missing resource、manual review。

## `tsuinosora.modern_profile_report.v1`

用于记录 modern profile 的增强项及回退证据。

```yaml
schema: tsuinosora.modern_profile_report.v1
base_conversion_report: classic.conversion
enabled_profiles:
  system_ui: true
  filters: true
  audio_repair: true
  translation_patch: false
  remake_portrait_overlay: false
features:
  - feature_id: modern.system.backlog
    kind: system_ui
    enabled: true
    fallback: classic
    affects_core_state: false
    evidence: scenario_hash_match
  - feature_id: modern.filter.integer_scale
    kind: filter
    enabled: true
    fallback: original_frame
    affects_core_state: false
    evidence: visual_review_required
diagnostics: []
```

所有 feature 必须说明是否影响 Core state。modern feature 只能影响 presentation、audio、timeline、system UI 或 localization overlay；如果会改变 route、save/replay、backlog 或 read-state，就必须被 release gate 阻断。

## `tsuinosora.manual_signoff.v1`

用于记录人工完整通关、听音、画面和 replacement review。

```yaml
schema: tsuinosora.manual_signoff.v1
profile: classic
reviewer_alias: local_reviewer
scenario_report_id: classic.full_route
checks:
  - check_id: manual.full_playthrough
    result: not_started
    evidence_note: no-commercial-content
  - check_id: manual.audio_listening
    result: not_started
    evidence_note: no-commercial-content
  - check_id: manual.visual_review
    result: not_started
    evidence_note: no-commercial-content
  - check_id: manual.alias_replacement
    result: not_started
    evidence_note: no-commercial-content
blockers: []
```

允许的 `result`：

| Result | Meaning |
| --- | --- |
| `pass` | 已完成复核，没有阻断项 |
| `pass_with_diagnostics` | 有差异记录，但不阻断目标 profile |
| `blocked` | 有缺失、错误或授权边界问题 |
| `not_started` | 还没有执行人工复核，不能作为验收完成证据 |

## Release Gate Join

release gate 需要读取四类 report，并按 profile 合并判断：

| Gate input | Blocks when |
| --- | --- |
| source inventory | source hash 不匹配、coverage 缺口未解释、出现本地路径 |
| conversion report | route 断裂、missing asset 未处理、command/source map 不完整 |
| modern profile report | feature 改写 Core state、fallback 缺失、translation/overlay 不能独立关闭 |
| manual signoff | 任一 required manual check 未通过或未执行 |
