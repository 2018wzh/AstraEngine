# Conversion Manifest

本页定义 TsuiNoSora modernization sample 的脱敏 report schema。字段约束转换器、VFS 插件和 release gate。当前仓库已有 `Tools/TsuiNoSora/tsuinosora_tools.py` 的公开 synthetic/local helper slice，能生成 inventory、direct-readable extract preflight、Director `imap`/`mmap` resource map preflight、受限 `XFIR` RIFF/RIFX exact wrapper reader、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、受限 RIFF/RIFX readable chunk report、cast source map report、script source map report、route graph report、visual reference、visual screenshot capture/comparison、Asset analysis、conversion、modern profile、mount policy、stage3 gate、local gate 和 NativeVN package input report；公开 synthetic patch Web bundle 已能读取脱敏 mount policy 并输出 `player.patch_direct_read` route check。完整商业 Director/Shockwave cast parser/source-map reader、完整 payload 转换和真实本地 patch direct-read 仍未完成。

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

## `tsuinosora.visual_reference_report.v1`

用于记录仓库内两张权威视觉参考图的脱敏证据。默认 Stage 3 gate 会把 `Examples/TsuiNoSora/Docs/Title.png` 和 `Examples/TsuiNoSora/Docs/Game.png` 视为固定证据，校验文件存在、PNG 可读、尺寸和 hash。

固定证据：

| logical id | file | dimensions | sha256 |
| --- | --- | --- | --- |
| `title` | `Title.png` | `1386x1040` | `sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca` |
| `game` | `Game.png` | `1403x1053` | `sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84` |

允许字段：

- `logical_id`、`file_name`、`dimensions`、`hash`、`allowed_regions`、`diagnostics`。
- report 只能写 hash、尺寸、区域 id、coverage、diagnostic 和 layout metric。
- 缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 必须 blocking。
- 不输出新商业截图、正文、音频、影片帧、本地绝对路径或可复原 payload。

## `tsuinosora.visual_screenshot_capture_report.v1`

用于记录 ignored `.local` 中原版和 Demo 同 checkpoint 截图的脱敏捕获证据。截图文件不进入 repo、package section 或 release report。

允许字段：

- `checkpoint_id`、`route_id`、`required`、`original.path`、`demo.path`、`hash`、`dimensions`、`nonblank`、`regions`、`automation`、`diagnostics`。
- `automation` 只能记录 `tsuinosora.visual_capture_automation_report.v1` 的 schema、configured、backend、session roles、checkpoint step count、step kinds、automation hash、`execution_status`、captured checkpoint count、screenshot count、same-run capture roles 和 transcript hash；不得写启动命令、工作目录、launch environment、窗口标题、输入文本、本地路径或截图 payload。
- 路径必须是 `local_work_root` 相对路径；不得记录本地绝对路径、用户名、截图 payload、OCR 文本、音频或影片。
- checkpoint 必须至少覆盖 title、main menu、first dialogue、background viewport、text window、speaker/name area、choice menu、save/load page、代表性 route CG/scene 和 ending。
- 缺 checkpoint、缺 `capture_automation`、自动采集未执行或失败、缺截图、路径不安全、PNG 不可读、blank frame、region id 不安全或 region 尺寸为空都必须 blocking。

## `tsuinosora.visual_comparison_report.v1`

用于记录同 checkpoint 原版/Demo 的确定性图像指标和视觉 review 结果。

允许字段：

- `checkpoint_id`、`route_id`、`region_id`、`dimensions`、`original_hash`、`demo_hash`、`mean_delta`、`changed_ratio`、`visual_review.status`、`visual_review.reviewer`、`visual_review.summary_hash`、`diagnostics`。
- 视觉 review 只能写摘要 hash；不得写商业截图、差异图、OCR 文本、正文、音频、影片或本地绝对路径。
- required checkpoint 缺 review、review fail、capture report blocked、尺寸不一致、region 越界、关键区域差异超过阈值或 path leak 都必须 blocking。

## `tsuinosora.projectorrays_converted_resources.v1`

用于让私有 full converter 把 ProjectorRays binary chunk 与真实 NativeVN output 建立可验证关系。该 sidecar 位于 ignored `local_work_root/reports/projectorrays_converted_resources.json`，由 `tsuinosora.projectorrays_full_dump_report.v1` 读取并重新校验。

允许字段：

- `source_alias`、`source_relative_path`、`source_sha256`、`chunk_fourcc`、`role`。
- `native_path`，必须位于 `native-assets/`。
- `converted_sha256`、`byte_size`、`conversion_method`、`status: converted`。

校验规则：

- `source_alias` 必须匹配 `projectorrays_full_dump_roots`，`source_relative_path` 必须指向该 root 下的 `.bin` chunk。
- `source_sha256` 必须匹配实际 chunk；`chunk_fourcc` 和 `role` 必须匹配 filename 与 `PROJECTORRAYS_REQUIRED_CHUNK_ROLES`。
- `native_path` 必须存在于 `local_work_root/native-assets/`，`converted_sha256` 和 `byte_size` 必须匹配实际文件。
- `conversion_method` 必须命名真实 converter；`hash_only`、`route_only`、`raw_chunk_copy`、`raw_copy` 和空 method 都必须 blocking。
- 当前 repo-side `projectorrays-convert-resources` 会按 chunk 类型写入真实 converter method：`projectorrays_json_metadata` 用于 paired JSON metadata chunk，输出 asset 只包含 schema、source hash、chunk role、metadata shape count 和 redaction，不复制 `name`、`scriptSrcText`、脚本文本或 raw JSON payload。
- `conversion_method: projectorrays_stxt_cp932_text` 用于 `STXT` chunk，要求 12-byte big-endian header、精确 text/trailer size 和 CP932 text payload，并只把解码文本写入 ignored `native-assets/`；report 仍不能写正文。
- `BITD` 通过 bitmap metadata、KEY/CASt binding 和可证明 palette sidecar 转成 RGBA PNG；`Lscr` 通过 source binding 转成 private script asset，empty metadata 只允许生成脱敏 no-op script metadata；`sndH`/`sndS` 通过 KEY-bound Moa PCM 转成 WAV；`ediM` `MACRZ` 只在 MP3 frame 连续覆盖到 EOF 且 header evidence 可验证时转成 MP3；`snd `、`cupt`、`SCRF`、`Cinf`、`VWFI`、`Sord`、`Fmap`、`VWLB`、`FCOL`、`FXmp`、`VERS`、`XTRl`、`VWSC`、`XMED` 等结构型 chunk 转成脱敏 metadata。不能用 metadata shape、hash-only、route-only 或 raw copy 计为 converted。
- 同一个 source chunk 只能出现一次；重复、未知 source、hash mismatch、缺 native asset 或路径泄露都必须 blocking。
- report 不得包含 raw ProjectorRays dump 内容、脚本文本、商业素材 payload、截图、音频、影片或本地绝对路径。

## `tsuinosora.extract_report.v1`

用于记录 unpack 前置预检。当前实现会复制合法可直接读取的 sidecar 图像、音频、影片、字体和文本；对带 Director `imap`/`mmap` 的 RIFF/RIFX container，会先按 resource map 读取有效 resource，避免把废弃 chunk 当作素材；对 `XFIR` 只接受已验证 exact wrapper 中的 RIFF/RIFX payload，并在 report 中记录 `container_format: XFIR`、`decoded_container_format`、`decoded_sha256` 和 `decoded_size`，不输出 decoded payload；对 `KEY*`/`CAS*` 生成脱敏 cast map，对 `Lctx`/`Lnam`/`Lscr` 生成脱敏 Lingo map；对可读或短 binary-header wrapped `Lscr` route marker 文本会生成脱敏 source-map sidecar；对没有 `imap` 的公开 fixture container，才退回受限线性 chunk 表读取。完整 Director/Shockwave cast/script/source-map reader 仍未完成；遇到 opaque/compressed `XFIR`、尾随未验证 bytes、`imap`/`mmap` 断裂、chunk 截断、没有可读 payload 或后续 coverage 无法证明的 container 时必须 blocking，不能把 sidecar、resource map 或 chunk 抽取视为完整解包。

```yaml
schema: tsuinosora.extract_report.v1
status: blocked
source_alias: original_install_root
output_alias: local_work_root/unpacked
input_file_count: 2
extracted_count: 1
skipped_count: 1
container_count: 1
container_entry_count: 1
protected_container_count: 1
format_counts:
  director_container: 1
  image_png: 1
containers:
  - relative_path: READY.dxr
    status: blocked
    container_format: RIFF
    form_type: MV93
    extraction_mode: director_resource_map
    director_resource_map:
      schema: tsuinosora.director_resource_map.v1
      status: pass
      imap_found: true
      resource_count: 1
      free_resource_count: 0
      tag_counts:
        CASt: 1
    entry_count: 1
    readable_payload_count: 0
    entries:
      - entry_id: ready.0001
        chunk_id: CASt
        chunk_offset: 12
        chunk_size: 128
        payload_sha256: sha256:redacted
        format_probe: unknown
        coverage_status: manual_review
files:
  - relative_path: Assets/bg.png
    output_relative_path: unpacked/Assets/bg.png
    size: 1024
    sha256: sha256:redacted
    format_probe: image_png
skipped:
  - relative_path: READY.dxr
    format_probe: director_container
    reason: director_reader_required
diagnostics:
  - code: TSUI_EXTRACT_DIRECTOR_READER_REQUIRED
    source_alias: original_install_root
    container_count: 1
redaction:
  paths: alias_or_report_relative_only
  payload: omitted
```

必须遵守：

- `containers`、`files` 和 `skipped` 只能使用 report-relative path；不得记录本地绝对路径、用户名、命令行实参或环境变量。
- report 只能记录 hash、offset、size、format probe、skipped reason、count、coverage 和 diagnostic；不得输出商业文本、截图、音频、影片或二进制 payload。
- `stage3-gate` 没有显式 `unpacked_root` 时可以自动运行该 preflight；只要 `status` 不是 `pass`，后续 NativeVN package input 必须阻断。

## `tsuinosora.director_resource_map.v1`

用于记录 Director RIFF/RIFX container 的 `imap`/`mmap` 预检。该 report 只证明有效 resource 的 FourCC、offset、size、hash 和 mmap 一致性；它不输出 resource payload，也不等价于完整 cast/script/source-map parser。

```yaml
schema: tsuinosora.director_resource_map.v1
status: pass
container_count: 1
resource_count: 2
free_resource_count: 1
tag_counts:
  PNG : 1
  Lscr: 1
containers:
  - relative_path: READY.dxr
    imap_found: true
    container_format: RIFF
    form_type: MV93
    endianness: little
    map_version: 1
    director_version: 1223
    mmap_offset: 4096
    mmap_header_size: 24
    mmap_entry_size: 20
    resource_count: 2
    free_resource_count: 1
    resources:
      - resource_id: 4
        tag: Lscr
        size: 128
        chunk_offset: 8192
        payload_sha256: sha256:redacted
        coverage_status: mapped
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
```

必须遵守：

- `resources` 只能记录 `resource_id`、FourCC、offset、size、flags、hash、coverage 和 diagnostic。
- `container_format: XFIR` 只表示 reader 验证并读取了 exact wrapper 中的 RIFF/RIFX payload；wrapper size 必须覆盖整个文件，report 可以记录 `decoded_container_format`、`decoded_sha256` 和 `decoded_size`，但不得写 decoded payload。
- RIFF/RIFX declared size 必须匹配可读文件大小；过大或过小都必须 blocking，不能继续抽取尾随或截断区域中的素材，也不能记录 resource/tag coverage。
- `mmap` free entry 只记录 `free_resource_count`；不得当作有效 resource、payload 或 tag coverage。
- `imap` 存在但 `mmap` offset、entry size、resource header 或 payload range 断裂时必须 blocking，不能退回线性 chunk 扫描。
- opaque、压缩、尾随未验证 bytes 或 payload 不是 RIFF/RIFX 的 `XFIR` 必须继续输出 reader-required blocking diagnostic。
- 没有 `imap` 的公开 synthetic fixture 可以退回 `linear_chunk_scan`，但正式商业 gate 仍需要后续 cast/source-map coverage 证明。

## `tsuinosora.director_cast_map.v1`

用于记录 Director `KEY*`/`CAS*` cast 关系预检。该 report 只读取 resource map 指向的 `KEY*`、`CAS*` 和 `CASt` resource，输出 cast member、library id、child resource FourCC 和 hash；当 `CASt` payload 内含显式 `tsuinosora.director_cast_member_metadata.v1` JSON 时，只提取 kind、route id、command id、anchor、bounds、character atlas parts 和 metadata hash。它不输出 cast payload，也不替代完整 Director cast/script/source-map parser。

```yaml
schema: tsuinosora.director_cast_map.v1
status: pass
container_count: 1
member_count: 1
containers:
  - relative_path: READY.dxr
    resource_map_status: pass
    key_table_count: 1
    cas_library_count: 1
    member_count: 1
    key_tables:
      - key_resource_id: 0
        entry_size: 12
        used_count: 2
        child_tag_counts:
          CAS*: 1
          PNG : 1
    cas_libraries:
      - cas_resource_id: 1
        library_resource_id: 1024
        cast_resource_count: 1
        cast_resource_ids_hash: sha256:redacted
    members:
      - member_id: ready.cast.1024.0
        source_container: READY.dxr
        cast_resource_id: 2
        cast_slot: 0
        library_resource_id: 1024
        cast_payload_sha256: sha256:redacted
        kind: background
        route_ids:
          - classic.main
        command_ids:
          - cmd.bg.title
        anchor:
          x: 0
          y: 0
        bounds:
          x: 0
          y: 0
          width: 640
          height: 480
        cast_metadata_schema: tsuinosora.director_cast_member_metadata.v1
        cast_metadata_sha256: sha256:redacted
        parts:
          - part_id: part.hero.neutral
            pose_id: pose.hero
            expression_id: neutral
            anchor:
              x: 16
              y: 64
            crop:
              x: 0
              y: 0
              width: 32
              height: 64
            layer: character
            mouth_eye_state_compatible: true
            fallback: nearest_pose
        child_resources:
          - resource_id: 3
            tag: "PNG "
            size: 128
            payload_sha256: sha256:redacted
            coverage_status: mapped
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
```

必须遵守：

- `KEY*` entry size 必须是 12 bytes，`used_count` 不能越界；否则 blocking。
- `CAS*` 只记录 cast resource id 的不可逆 hash 和计数；不输出原始 table payload。
- `CASt` 默认只记录 `cast_payload_sha256`；只有显式 `tsuinosora.director_cast_member_metadata.v1` JSON 才能贡献 kind、route id、command id、anchor、bounds 和 atlas parts，这些字段必须是脱敏 symbol、boolean 或数值。
- `CASt` metadata 不得包含正文、payload 或 bytecode 字段；blocking diagnostic 只能记录字段路径，不记录字段值。
- anchor 必须包含数值 `x`/`y`，bounds 必须包含非负数值 `x`/`y`/`width`/`height`；类型不明或负尺寸必须 blocking，不能静默丢弃布局证据。
- `kind: character_atlas` 必须包含 parts；part id、pose id、expression id、layer 和 fallback 必须是 safe symbol，anchor 和 crop 必须是可验证数值，mouth/eye state compatibility 必须是 boolean。
- child resource 只记录 id、FourCC、size、hash 和 coverage。
- 同一个 `CASt` resource 被多个 `CAS*` library/slot 绑定时必须 blocking，不能静默选择第一条绑定。
- 该 report 可以证明 cast/resource 关系存在，但完整商业转换仍需要把 child resource 类型、atlas 切片和 route/source map 一起闭合。

## `tsuinosora.director_lingo_map.v1`

用于记录 Director `Lctx`/`Lnam`/`Lscr` Lingo 资源预检。该 report 只记录 Lingo context、name table 和 script chunk 的 resource id、entry id、size、hash、文本可抽取性和是否需要 bytecode reader；不输出 Lingo name 字符串、脚本文本或 bytecode。

```yaml
schema: tsuinosora.director_lingo_map.v1
status: pass
container_count: 1
context_count: 1
context_entry_count: 1
name_count: 1
name_entry_count: 2
script_count: 1
unsupported_script_count: 1
containers:
  - relative_path: READY.dxr
    resource_map_status: pass
    context_count: 1
    context_entry_count: 1
    name_count: 1
    name_entry_count: 2
    script_count: 1
    unsupported_script_count: 1
    resources:
      - resource_id: 2
        entry_id: ready.0002
        tag: Lctx
        size: 4
        payload_sha256: sha256:redacted
        entry_count: 1
        entry_table_sha256: sha256:redacted
      - resource_id: 3
        entry_id: ready.0003
        tag: Lnam
        size: 32
        payload_sha256: sha256:redacted
        entry_count: 2
        entry_table_sha256: sha256:redacted
      - resource_id: 4
        entry_id: ready.0004
        tag: Lscr
        size: 128
        payload_sha256: sha256:redacted
        coverage_status: mapped
        script_text_extractable: false
        script_text_extracted: false
        requires_bytecode_reader: true
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
  lingo_names: omitted
  bytecode: omitted
```

必须遵守：

- `Lnam` 只能输出 table hash、entry count 和 resource count，不得输出 symbol/name 字符串。
- `Lctx` 只能输出 table hash、entry count 和 resource count，不得输出 context payload。
- `Lctx` payload size 必须按 32-bit entry 对齐；未对齐时 `tsuinosora.director_lingo_map.v1` 必须 blocking。
- 当前内置 `Lnam` preflight 只接受 null-terminated sanitized name table；未终止或无法证明 name 边界时必须 blocking，留给专用 reader 处理。
- `Lscr` 如果不是可直接抽取的文本，也不是短 binary-header 后接可解码 route marker 文本，`requires_bytecode_reader` 必须为 `true`；后续 `tsuinosora.script_source_map_report.v1` 必须 blocking，除非合规 `tsuinosora.script_source_map.v1` sidecar 用同一 `director_lingo_map.json` source 和匹配 `sha256` 证明 route coverage。
- 该 report 只证明 Lingo 资源存在和是否需要完整 reader，不替代 Director/Shockwave Lingo bytecode decompiler。

## `tsuinosora.cast_source_map_report.v1`

用于把解包素材映射到脱敏 cast member、source hash、route id 和 command id。当前实现接受 `tsuinosora.cast_map.v1` sidecar、RIFF/RIFX metadata JSON chunk，或从 `tsuinosora.director_cast_map.v1` 通过 child resource id、FourCC、container entry id 和 extracted payload hash 派生；如果 director cast member 已携带脱敏 metadata，`kind`、`route_ids`、`command_ids` 和 `parts` 会继续传入 source-map member。它验证 source relative path、container entry id 和 source hash，不输出原始 cast payload。

```yaml
schema: tsuinosora.cast_source_map_report.v1
status: pass
source_count: 1
member_count: 1
sources:
  - source: containers/ready/0002_cmap.json
    sha256: sha256:redacted
    member_count: 1
members:
  - member_id: cast.bg.title
    kind: background
    source: containers/ready/0001_png.png
    source_hash: sha256:redacted
    container_entry_id: ready.0001
    director_child_resource_id: 3
    director_child_tag: "PNG "
    director_child_payload_sha256: sha256:redacted
    route_ids:
      - classic.main
    command_ids:
      - cmd.bg.title
    coverage_status: mapped
    map_source: containers/ready/0002_cmap.json
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
```

`stage3-gate` 在存在 `unpacked_root` 时必须生成该 report。缺 cast map、member source 缺失、sidecar 声明的 `source_hash` 与实际 extracted source asset 不一致、Director child resource 未抽取、hash 冲突、kind 不在分类集合内、symbol/path 不安全、sidecar 带正文/payload/bytecode 字段或 report 出现路径泄露时 blocking。手写 `tsuinosora.cast_map.v1` 和外部 `tsuinosora.director_cast_map.v1` sidecar 都只能写脱敏 member/source/hash/route/command evidence；payload 类 diagnostic 只记录字段路径，不记录字段值。Director-derived member 必须保留 child resource id、FourCC、container entry id 和 payload hash，证明素材来源不是靠文件名猜测。这个 report 证明素材和 cast/source map 的脱敏对应关系；正式商业转换仍需要完整 Director/Shockwave cast parser。

## `tsuinosora.route_graph_report.v1`

用于从已解包的 route metadata 生成自动化路线覆盖证据。当前实现只接受脱敏 `tsuinosora.route_graph.v1` JSON，不解析商业脚本文本；route id、terminal 和 choice id 必须是 safe symbol，sidecar 不得包含正文、bytecode 或 payload 字段。完整 Director/Shockwave Lingo/cast route extraction 仍是 Stage 3 缺口。

```yaml
schema: tsuinosora.route_graph_report.v1
status: pass
source_count: 1
route_count: 1
sources:
  - source: route_graph.json
    route_count: 1
    sha256: sha256:redacted
routes:
  - route_id: classic.main
    coverage: covered
    terminal: ending.good
    choices:
      - choice.start
    source: route_graph.json
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
```

`stage3-gate` 在没有显式 `routes` 输入时会尝试从 `unpacked_root` 生成该 report；缺 route、route 未覆盖、route id/terminal 不完整、choice id 不安全、同一 `route_id` 映射到多个 terminal/choice signature、同一 route 内重复 choice id、sidecar 带正文类字段或 report 出现路径泄露时必须 blocking。只有 route graph 缺失时才允许继续尝试 `tsuinosora.script_source_map_report.v1` fallback；如果 route graph sidecar 存在但校验失败，script source-map fallback 不能覆盖或消除该 blocking diagnostic。该 report 只记录 route id、terminal id、choice id、source relative path、hash 和 diagnostic，不记录脚本文本。

## `tsuinosora.script_source_map.v1`

外部 Director/Lingo reader 可以把已验证的 route source map 写成该脱敏 sidecar。它是 `tsuinosora.script_source_map_report.v1` 的输入，不是发布 package section，也不表示仓库已经内置完整 Lingo decompiler。sidecar 只能写 reader identity、reader output hash、source relative path、source hash、line、route id、terminal id、choice id、coverage、`script_resource_id` 和 `script_payload_sha256`；route source 必须匹配同一 sidecar 中声明的 source，route `source_hash` 必须等于该 source 的 `sha256`；当 source 指向 `director_lingo_map.json` 且该 map 中存在 unsupported `Lscr` bytecode 时，route 必须同时声明匹配的 `script_resource_id` 和 `script_payload_sha256`，才能作为 route coverage 证据；不得写 `text`、`script_text`、`source_text`、`content`、`payload`、`bytecode` 或本地路径。

```yaml
schema: tsuinosora.script_source_map.v1
reader:
  tool_id: tonguetwister.lingo-reader
  tool_hash: sha256:redacted
  output_contract: route_source_map
sources:
  - source: containers/ready/director_lingo_map.json
    sha256: sha256:redacted
    line_count: 12
    script_count: 1
routes:
  - route_id: classic.main
    coverage: covered
    terminal: ending.good
    choices:
      - choice.start
    source: containers/ready/director_lingo_map.json
    line: 7
    source_hash: sha256:redacted
    script_resource_id: 4
    script_payload_sha256: sha256:redacted
```

`Tools/TsuiNoSora` 会拒绝带正文类字段、绝对路径、非 `sha256:` evidence、未声明 source、声明 source hash 与现有 report-relative source 文件不一致、route/source hash 不一致、route line 超出声明 source line_count、非 covered route、不安全 symbol、缺失 `script_resource_id`/`script_payload_sha256`、未知 `script_resource_id` 或 Lscr payload hash mismatch 的 sidecar，并且 diagnostic 只写字段名、sidecar 相对路径、resource id 和错误代码，不写 resource payload。

## `tsuinosora.script_source_map_report.v1`

用于从可读脚本文本或脱敏 source-map sidecar 生成 route coverage。当前实现支持 sidecar script、RIFF/RIFX text chunk 中的显式 route marker、`tsuinosora.script_source_map.v1` reader sidecar，并读取 `tsuinosora.director_lingo_map.v1` 的 bytecode preflight；当 source 指向 unsupported `Lscr` bytecode 所在的 `director_lingo_map.json` 时，会校验 route 的 `script_resource_id` 和 `script_payload_sha256` 是否匹配该 Lscr resource。它不输出商业脚本文本，也不替代完整 Lingo/cast 反编译。

```yaml
schema: tsuinosora.script_source_map_report.v1
status: pass
source_count: 1
route_count: 1
reader_count: 1
readers:
  - source_map: maps/script_source_map.json
    tool_id: tonguetwister.lingo-reader
    tool_hash: sha256:redacted
    output_contract: route_source_map
sources:
  - source: containers/ready/0002_lscr.ls
    sha256: sha256:redacted
    line_count: 12
    route_marker_count: 1
routes:
  - route_id: classic.main
    coverage: covered
    terminal: ending.good
    choices:
      - choice.start
    source: containers/ready/0002_lscr.ls
    line: 4
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
  commercial_text: omitted
```

`stage3-gate` 在没有显式 `routes` 且 `tsuinosora.route_graph_report.v1` 缺失时，会尝试生成该 report。缺 marker、reader sidecar 不合规、reader id/hash/output contract 不安全、route source/hash 断裂、route line 超出声明 source line_count、同一 `route_id` 映射到多个 terminal/choice signature、同一 route 内重复 choice id、source map 路径泄露、声明 source hash 与现有 report-relative source 文件不一致、unsupported Lingo bytecode 没有合规 sidecar 覆盖、sidecar 未声明匹配的 `script_resource_id`/`script_payload_sha256`、同一 `director_lingo_map.json` 的 unsupported `Lscr` resource 未被逐个覆盖或无法证明 coverage 时必须 blocking。route marker 和 `tsuinosora.script_source_map.v1` 是公开 fixture 与本地 reader 中间层约定，正式商业转换仍需要 Director/Shockwave script/cast source map reader。

## `tsuinosora.asset_analysis.v1`

用于在 unpack 之后、写入 `native-assets/` 之前阻断素材误分类。当前 helper 只输出脱敏证据，不输出图像 payload。

```yaml
schema: tsuinosora.asset_analysis.v1
status: pass
classification_counts:
  background: 1
duplicate_hashes: []
assets:
  - relative_path: bg/title.png
    classification: background
    confidence: 0.88
    sha256: sha256:redacted
    container_source: bg
    script_references:
      - source: Scripts/main.astra
        line: 12
        reference_kind: background
    use_timing: story_route
    dimensions:
      width: 640
      height: 480
    has_alpha: false
    visible_bbox:
      x: 0
      y: 0
      width: 640
      height: 480
    edge_padding:
      left: 0
      top: 0
      right: 0
      bottom: 0
    color_distribution:
      - rgb_bin: "#404080"
        coverage: 0.42
    reference_matches: []
    parts: []
quarantine: []
diagnostics: []
```

必须覆盖：

- `character_atlas` 的 crop/part、pose/expression、anchor、layer、mouth/eye state 和 fallback。
- `ui`、`text_window`、`button`、`background` 和 `character_sprite` 的分类冲突阻断。
- 重复 hash、透明通道、edge padding、颜色分布和参考截图匹配。
- 脚本引用位置只记录相对 source 和 line，不记录脚本文本。

## `tsuinosora.native_asset_rearrange_report.v1`

用于证明 Asset analysis 通过后，转换器确实把解包资产写入本地 `native-assets/` 布局。该 report 不进入公开 package，只作为本地 conversion gate evidence；字段只能是相对路径、classification、hash、byte size、coverage 和 diagnostic。

```yaml
schema: tsuinosora.native_asset_rearrange_report.v1
status: pass
output_root: local_work_root/native-assets
converted_assets: 1
resources:
  - source: containers/ready/0001_png.png
    native_path: native-assets/backgrounds/containers/ready/0001_png.png
    classification: background
    source_hash: sha256:redacted
    converted_hash: sha256:redacted
    byte_size: 4096
    coverage_status: converted
diagnostics: []
redaction:
  paths: report_relative_only
  payload: omitted
```

如果 `tsuinosora.asset_analysis.v1` blocked、source 不存在、classification 是 `unknown`/不支持值、输出路径不安全或 copy 后 hash 不一致，该 report 必须 `blocked`，并且 `tsuinosora.conversion_report.v1` 也必须 `blocked`。

## `tsuinosora.conversion_report.v1`

用于记录解包、重排、NativeVN 转换和缺失资源。

```yaml
schema: tsuinosora.conversion_report.v1
status: pass
inputs:
  original_install_root: original_install_root
counts:
  source_files: 8
  asset_count: 1
  quarantine_count: 0
  route_count: 0
  converted_assets: 0
  missing_assets: 0
routes:
  - route_id: classic.main
    coverage: covered
    terminal: ending.good
    choices: [choice.start]
    mount_assets:
      - alias: asset.cast_bg_title
        path: native-assets/backgrounds/containers/ready/0001_png.png
        role: background
        route_id: classic.main
        sha256: sha256:redacted
resources:
  - source: containers/ready/0001_png.png
    native_path: native-assets/backgrounds/containers/ready/0001_png.png
    classification: background
    source_hash: sha256:redacted
    converted_hash: sha256:redacted
    byte_size: 4096
    coverage_status: converted
diagnostics: []
```

必须覆盖：

- route coverage、terminal、choice/source-map 证明。
- route-bound cast member 到 native-assets 的 mount evidence；只能使用 source hash、converted hash、classification 和 route id，不得靠文件名猜测。
- Asset analysis quarantine、native-assets rearrange status、converted/missing asset count。
- source/native 相对路径、source hash、converted hash、classification 和 byte size。
- missing resource、manual review、path leak、payload leak 和 hash mismatch blocking diagnostic。

`resources` 通过时必须至少包含一条 converted resource evidence；如果所有 routes 都是 `covered` 但 `resources` 为空，release gate 必须 blocking，不能把 route coverage 当作素材转换完成证据。每条 resource 必须包含 source/native 相对路径、classification、source hash、converted hash 和正 byte size；hash 必须使用 `sha256:` 前缀。

## `tsuinosora.nativevn_package_input_report.v1`

用于记录本地转换写出的 NativeVN package 输入。该 report 只列出相对路径、role、`sha256` 和 byte size，用来证明 `project.yaml`、`.astra` story、package section input 和 scenario refs 已真实写出；不得输出 story 正文、商业素材 payload 或本地绝对路径。

```yaml
schema: tsuinosora.nativevn_package_input_report.v1
status: pass
project_root: local_work_root/nativevn
project: nativevn/project.yaml
story: nativevn/Scripts/main.astra
section_count: 6
scenario_count: 12
route_count: 1
files:
  - role: project
    path: nativevn/project.yaml
    sha256: sha256:redacted
    byte_size: 1024
  - role: story
    path: nativevn/Scripts/main.astra
    sha256: sha256:redacted
    byte_size: 2048
  - role: package_section
    path: nativevn/PackageSections/asset_analysis.json
    section_id: tsuinosora.asset_analysis
    section_schema: tsuinosora.asset_analysis.v1
    sha256: sha256:redacted
    byte_size: 4096
  - role: scenario_ref
    path: nativevn/scenarios/tsuinosora-internal-game.classic.headless.classic_main.json
    sha256: sha256:redacted
    byte_size: 512
diagnostics: []
redaction:
  paths: report_relative_or_alias_only
  payload: omitted
  commercial_text: omitted
```

`files` 在通过时必须至少覆盖 `project`、`story`、`package_section` 和 `scenario_ref`。只要 conversion report、Asset analysis 或 route coverage blocked，该 report 必须保持 blocked；如果 story/scenario refs 未写出，`project` 和 `story` 字段必须为空，`files` 不能登记不存在的 project/story。

NativeVN package input 生成 `.astra` story 和 scenario refs 时，必须保留 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 中的 sanitized `choices`。`.astra` option key 和 scenario `player_input choose.value` 必须使用同一个 choice id；只有 route 没有 choice 证据时才允许生成 fallback choice id。多 choice route 必须按 source-map 顺序生成连续 choice state，不能压缩成单个 synthetic choice。显式传入的 routes 也必须在写 story/scenario 前重新校验；不安全 `route_id`/terminal/choice、非 covered coverage、重复 choice 或冲突 route signature 都必须 blocking，且 `scenario_count` 为 0。`stage3-gate` 从 cast source map 和 native asset rearrange report 派生出的 route-bound `mount_assets` 必须写入 conversion report，并在没有显式 routes 入参时继续进入 NativeVN scenario refs，不能在 `local-gate` 里丢失。

## `tsuinosora.local_gate_report.v1`

用于把 `stage3-gate` 与 NativeVN package input 写入串成一个本地执行入口。该 report 只记录 alias、相对 report path、target matrix、route count 和 diagnostic。`route_count` 必须来自实际写出的 `tsuinosora.nativevn_package_input_report.v1`，包括从 `stage3-gate` 派生出的 route，不能只复述 CLI 入参。`local-gate` 不接受显式 routes 作为商业 route coverage；如果调用方传入 routes 但没有 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生证据，report 必须用 `TSUI_LOCAL_GATE_ROUTE_EVIDENCE_REQUIRED` 阻断，并且 `nativevn_package_input` 为空。

```yaml
schema: tsuinosora.local_gate_report.v1
status: pass
reports:
  stage3_gate: reports/stage3_gate_report.json
  nativevn_package_input: reports/nativevn_package_input_report.json
route_count: 1
diagnostics: []
```

如果 `stage3_gate_report` blocked，local gate 不写 NativeVN package input，并输出 `TSUI_LOCAL_GATE_STAGE3_BLOCKED`。如果调用方显式传入 routes，local gate 也不写 NativeVN package input；该场景必须先落成 route graph 或 script source-map report。

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

formal release gate 会把 `manual.full_playthrough`、`manual.audio_listening`、`manual.visual_review` 和 `manual.alias_replacement` 当作 required check set，并且 required check 必须使用 `check_id` 字段；任一项缺失、`not_started`、`blocked` 或存在 blocker，`tsuinosora.manual_signoff` 都必须 blocking。report 只能写 check id、result、blocker count 和脱敏 note，不能写正文、截图、音频、影片或本地路径。

## Release Gate Join

release gate 需要读取四类 report，并按 profile 合并判断：

| Gate input | Blocks when |
| --- | --- |
| source inventory | source hash 不匹配、coverage 缺口未解释、出现本地路径 |
| conversion report | route 断裂、missing asset 未处理、command/source map 不完整 |
| modern profile report | feature 改写 Core state、fallback 缺失、translation/overlay 不能独立关闭 |
| manual signoff | 任一 required manual check 未通过或未执行 |

所有进入 package 的 `tsuinosora.*` section 还会统一扫描 payload-like 字段。`text`、`script_text`、`source_text`、`content`、`payload`、`payload_bytes`、`bytecode`、`bytes`、`commercial_text`、`lingo_source`、`raw_payload` 和 `source_payload` 出现在 report 主体时必须 blocking；唯一允许的 `payload` 键是 `redaction.payload: omitted`。
