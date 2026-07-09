# Implementation Plan

本页是 Stage 3 的执行蓝图。当前仓库已有 AstraVN 多 crate 基础 slice、NativeVN runtime provider、VN package/release gate slice，以及 `Tools/TsuiNoSora` 的脱敏 inventory、direct-readable extract preflight、Director `imap`/`mmap` resource map preflight、受限 `XFIR` RIFF/RIFX exact wrapper reader、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、从 Director cast map 与 child resource id/FourCC/extracted payload hash 派生 cast source map、受限 RIFF/RIFX readable chunk reader、opaque/compressed `XFIR` 与尾随未验证 bytes reader-required 阻断、ProjectorRays 本地 dump adapter、ProjectorRays full dump coverage report、JSON-backed metadata chunk converter、`STXT` text converter、`Lscr` cast-member/source-number/CastScript/ParentScript source 映射和 malformed JSON numeric recovery、empty `Lscr` no-op metadata converter、`BITD` 1/16/32bpp PNG converter、8bpp `BITD` palette sidecar converter、KEY-bound `sndH`/`sndS` WAV converter、KEY-bound `ediM` `MACRZ` verified MP3 converter、score/metadata chunk 脱敏 converter、ProjectorRays `GO[...]` route identity 派生、ProjectorRays converted asset bridge、可读 mapped `Lscr` 自动生成脱敏 source-map sidecar、script source map report、route graph report、visual reference report、visual screenshot capture/comparison report、Asset analysis、conversion report、modern profile report、route scenario、mount policy helper、NativeVN package input writer、NativeVN asset sidecar、project-level `package_sections`、scenario refs package section、TsuiNoSora package section release gate、公开 synthetic internal/patch player route gate、公开 synthetic patch Web direct-read route check，以及 `player.full_playable` report 校验。`S3-TSUI-INTERNAL-DEMO-01` 的 repo-side pipeline 已落地，私有 `.local` full dump 已能生成 `tsuinosora.projectorrays_full_dump_report.v1`；当前私有 conversion 已覆盖 2527/2527 个 ProjectorRays binary chunk，`demo-slice` 可生成 28 条脱敏 route 的 NativeVN project/package input，`internal-demo-bundle` 已从同一个 `.astrapkg` 产出 Windows/Web bundle manifest。真实 internal playable acceptance 尚未完成；当前 blocker 是缺少私有 `visual_capture` 原版/Demo 截图配置、同 run Windows live input/audio/route automation evidence 和 required manual signoff。Stage 3 总体仍是 `IN_PROGRESS`。formal release signoff、modern、Patch-only 和 Runtime Patch/VFS 插件仍是后续计划，不能把本页写成 Stage 3 `DONE` 证据。

阶段顺序固定为：先用本地完整转换包验证 NativeVN 复刻，再实现现代化和补丁式本地挂载应用。仓库不提交原始文件、解包文件、转换后商业 payload、文本正文、截图、音频或影片。

## Planned Game Targets

Stage 3 需要两个 planned `Game` target。本期只交付 internal classic full-resource demo bundle，其他 target/profile 不算本期完成条件：

| Target | Purpose | Planned automation |
| --- | --- | --- |
| `tsuinosora-internal-game` | 本地完整转换包，用于证明 AstraEngine + AstraVN 能承载真实商业 VN 项目 | 本期：`classic` profile 完成全量资源转换、原体验还原、NativeVN project、package、Windows/Web standalone bundle 和 live automation report evidence；后续：modern profile |
| `tsuinosora-patch-game` | 补丁式本地挂载应用，只分发应用、插件、补丁、mount policy 和脱敏 manifest | 后续计划；classic/modern profile 的 Windows/Web direct-read 与 Runtime Patch/VFS 插件不属于本期 |

`internal` target 可以读取 `local_work_root` 下的本地生成产物。`patch` target 只能通过用户本机选择的合法数据根挂载原版和可选 Remake 资源。两个 target 的 package 和 report 都不能记录本地绝对路径。本期所有私有输入、ProjectorRays 输出、中间 NativeVN project、package 和 bundle 都放在 `Examples/TsuiNoSora/.local/` 或等价 ignored 工作区。仓库提供可提交模板 `Examples/TsuiNoSora/Docs/demo.config.template.json`，也可以用下面的命令在 ignored 工作区生成本地 config scaffold：

```bash
python Tools/TsuiNoSora/tsuinosora_tools.py demo-config-template \
  --out Examples/TsuiNoSora/.local/demo.config.json
```

私有验收入口为：

```bash
python Tools/TsuiNoSora/tsuinosora_tools.py internal-demo-bundle \
  --config Examples/TsuiNoSora/.local/demo.config.json \
  --repo-root . \
  --player-automation-report Examples/TsuiNoSora/.local/work/reports/live_player_report.json
```

该命令只在 full-resource ProjectorRays conversion coverage、`demo-slice`、cook/package、Windows/Web bundle manifest、`player.full_playable` release check 和 required visual screenshot comparison 都通过时输出 `tsuinosora.internal_demo_bundle_report.v1 status: pass`；缺 `.local` config、缺 ProjectorRays tool/dump、缺私有源、缺 converted resource evidence、缺 live report、缺 checkpoint 截图、blank frame、关键区域 mismatch、缺视觉 review、缺 `capture_automation` 自动采集 intent、required checkpoint 缺 same-run original/demo capture role 或 `execution_status` 不是 `pass` 时必须 blocking。模板和 scaffold 只写 repo-relative `.local` 示例路径，不写本地绝对路径；真实 Windows/Web bundle 仍需要把合法私有源、ProjectorRays dump、converted NativeVN assets、live player report 和 visual capture/review evidence 填入 ignored config。

## Phase 1 Source Inventory

输入是 `original_install_root` 和 `remake_install_root`。工具只读取用户本地合法文件，输出到 `local_work_root/inventory/`，生成 `tsuinosora.source_inventory.v1`。

必须记录：

- source profile：`original_1999`、`remake_portrait_overlay`。
- logical id、resource kind、size、hash、container、coverage status。
- archive/cast/movie 计数、未知资源计数、解析诊断码。
- 无法读取或需要人工复核的资源，不记录本地路径。

Remake 数据在 Stage 3 中用于完整 modern profile 验证，第一阶段只读取立绘 overlay 候选，不默认替换背景、CG、UI、影片或音频。

## Phase 2 Reference Evidence

`Examples/TsuiNoSora/Docs/Title.png` 和 `Examples/TsuiNoSora/Docs/Game.png` 是原版表现复刻的权威视觉参考。它们只作为仓库内已有参考证据使用，不新增商业截图输出。当前工具会把默认参考图的尺寸和 hash 作为固定证据校验：`Title.png` 必须是 `1386x1040`、`sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca`，`Game.png` 必须是 `1403x1053`、`sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84`。

Stage 3 需要生成 `tsuinosora.visual_reference_report.v1`，只记录：

- reference logical id、文件名、尺寸、hash 和用途。
- comparison region id，例如 title background、title menu、dialogue viewport、frame border、text window、speaker label、text baseline。
- layout metric、区域 hash、coverage、diagnostic code 和 pass/block 状态。

`Title.png` 用于校验标题背景、菜单按钮、选中态、整体色彩和构图。`Game.png` 用于校验背景视窗、边框、文本窗、speaker/text 位置和原版画幅比例。report 不得写入新的截图 payload、文本正文、音频采样、影片帧或本地绝对路径。

缺参考图、PNG 不可读、hash mismatch 或 dimensions mismatch 都是 blocking diagnostic，不能继续把视觉参考证据写成 `pass`。

## Phase 2A Screenshot Visual Acceptance

私有 config 的 `visual_capture` 定义原版和 Demo 的同 checkpoint 截图、区域、阈值和 `capture_automation` 自动采集 intent；截图文件只写入 ignored `local_work_root/screenshots/`。`capture_automation` 可包含私有启动命令、工作目录、launch environment、窗口匹配和输入脚本，`visual-capture` 和 `internal-demo-bundle` 会用 Windows `SendInput`/GDI backend 启动原版与 Demo、发送真实输入并在 checkpoint 写出截图；`tsuinosora.visual_screenshot_capture_report.v1` 只输出 backend、session role、checkpoint step count、automation hash、`execution_status`、captured checkpoint count、screenshot count、same-run capture roles 和 transcript hash，不写启动命令、launch environment、窗口标题、脚本输入值、本地路径或截图 payload。仓库侧工具提供两条脱敏报告命令：

```bash
python Tools/TsuiNoSora/tsuinosora_tools.py visual-capture \
  --work-root Examples/TsuiNoSora/.local/work \
  --config Examples/TsuiNoSora/.local/visual_capture.json

python Tools/TsuiNoSora/tsuinosora_tools.py visual-comparison \
  --work-root Examples/TsuiNoSora/.local/work \
  --capture-report Examples/TsuiNoSora/.local/work/reports/visual_screenshot_capture_report.json \
  --visual-reviews Examples/TsuiNoSora/.local/visual_reviews.json
```

`tsuinosora.visual_screenshot_capture_report.v1` 和 `tsuinosora.visual_comparison_report.v1` 只能记录 checkpoint id、route id、region id、尺寸、hash、差异指标、自动采集 intent/execution 摘要、视觉 review hash 和 diagnostic。缺原版截图、缺 Demo 截图、PNG 不可读、blank frame、区域越界、尺寸不一致、required review 缺失、review fail、缺 `capture_automation`、自动采集执行失败、required checkpoint 缺 same-run original/demo capture role 或关键区域差异超过阈值都会 blocking。视觉截图、差异图、调试视频和任何商业画面不得进入 repo、package section 或 release report。

## Phase 3 Unpack

本地转换工具把原版 Director/Shockwave 资源解包到 `local_work_root/unpacked/`。该目录是本地生成产物，不进入仓库。当前 `Tools/TsuiNoSora/tsuinosora_tools.py extract-readable` 已落地的是阻断式 direct-readable preflight：复制合法可直接读取的 sidecar 图像、音频、影片、字体和文本；对带 `imap`/`mmap` 的 RIFF/RIFX Director container 先读取 resource map，只处理 mmap 指向的有效 resource；`XFIR` 只接受已验证 exact wrapper 中的 RIFF/RIFX payload，wrapper size 必须覆盖整个文件，并继续按 Director resource map 读取，report 只记录 decoded hash/size，不输出 decoded payload；opaque、压缩、尾随未验证 bytes 或无法证明 source/hash 的 Shockwave container 仍输出 reader-required blocking diagnostic，不退回线性扫描；对含 `KEY*`/`CAS*` 的 mapped container 会写出脱敏 `tsuinosora.director_cast_map.v1` 元数据，并且只在 `CASt` payload 显式包含 `tsuinosora.director_cast_member_metadata.v1` 时提取 kind、route id、command id、anchor、bounds、character atlas parts 和 metadata hash，后续 `tsuinosora.cast_source_map_report.v1` 只能通过 child resource id、FourCC、container entry id 和 extracted payload hash 派生；对含 `Lctx`/`Lnam`/`Lscr` 的 mapped container 会写出脱敏 `tsuinosora.director_lingo_map.v1`，只记录 Lingo resource id、entry id、hash、可直接抽取文本标志和 bytecode reader 需求；当 mapped `Lscr` 可直接抽取为文本，或带短二进制 header 但后续存在可解码 route marker 文本时，会额外写出 `director_lingo_source_map.json`，该 sidecar 只记录 reader id/hash、`director_lingo_map.json` source/hash、line、route id、terminal、choice 和 coverage；对没有 `imap` 的公开 fixture container 才退回受限线性 chunk 表读取；只抽取带公开文件签名的 embedded payload 到 `local_work_root/unpacked/`，同时生成 `tsuinosora.extract_report.v1` 和嵌套 `tsuinosora.director_resource_map.v1` 摘要。`director-resource-map`、`director-cast-map` 与 `director-lingo-map` 子命令可以单独输出脱敏 preflight。如果 container 无法识别、`imap`/`mmap` 断裂、chunk 截断、没有可读 payload，或缺少后续 source-map/route 证据，report 和 conversion 必须 blocking，不能把可读 resource 抽取误写成完整 Director/Shockwave 转换。

ProjectorRays 是本地 reader 来源，由 ignored config 指向本机 tool path 和 dump root。本仓不自动下载、不提交第三方源码或二进制；adapter 只读取 ProjectorRays 已生成的本地 dump，输出 `tsuinosora.projectorrays_reader_report.v1` 和脱敏 `tsuinosora.script_source_map.v1`。这些 report 只能记录 reader id/hash、dump 相对 source、source hash、line count、route id、terminal、choice 和 coverage，不得写脚本文本、payload、bytecode、ProjectorRays 原始 dump 内容或本地绝对路径。在 `demo-slice` 中，ProjectorRays pass report 只满足 Director reader-required preflight；payload-like 字段、路径泄露、hash/source 断裂、resource coverage 缺失和 route/source-map 缺失仍会 blocking。若 dump 无法派生 route/source-map evidence，Demo gate 必须 blocking，不能手写 routes 绕过。

可先运行 metadata converter 生成 private converted evidence：

```bash
python Tools/TsuiNoSora/tsuinosora_tools.py projectorrays-convert-resources \
  --work-root Examples/TsuiNoSora/.local/work \
  --dump-root root=Examples/TsuiNoSora/.local/projectorrays-full-root \
  --dump-root data=Examples/TsuiNoSora/.local/projectorrays-full-data \
  --dump-root casts=Examples/TsuiNoSora/.local/projectorrays-full-casts \
  --palette-sidecar Examples/TsuiNoSora/.local/palettes/system-win-d5.palette.json
```

该命令会把 ProjectorRays paired JSON metadata chunk 转成脱敏 `native-assets/projectorrays/...` metadata asset，把 `STXT` 解码为 private UTF-8 text asset，把可证明通过 cast member、same-scope `scriptNumber`、`CastScript`/`ParentScript` source 或 malformed JSON numeric recovery 绑定到 `.ls/.lasm` 的非空 `Lscr` 转成 private script asset，把 empty ProjectorRays `Lscr` metadata 转成脱敏 no-op script metadata，通过 `KEY_`/bitmap `CASt` metadata 把 1/16/32bpp `BITD` 转成 RGBA PNG；如果提供合规 `tsuinosora.projectorrays_palette_sidecar.v1`，还会把 matching stored clut id 的 8bpp `BITD` 转成 PNG，report 只记录 palette id、sidecar hash 和 entry count，不写 palette 路径或颜色表。它还会把 empty `snd `、zero `cupt`、`SCRF`、`Cinf`、`VWFI`、`Sord`、`Fmap`、`VWLB`、`FCOL`、`FXmp`、`VERS`、`XTRl`、`VWSC` 和 `XMED` 转成脱敏结构 metadata，把 KEY-bound `sndH`/`sndS` Moa PCM 转成 WAV audio asset，并把 KEY-bound `ediM` `MACRZ` 媒体解析为经 frame 边界校验的 MP3 audio asset，report 只记录 stream offset/byte size/hash、frame count、sample rate、bitrate、channel count、marker/GUID hash 和 diagnostic。`--summary` 只输出脱敏计数，完整 sidecar 仍写入 ignored `reports/projectorrays_converted_resources.json`。8bpp `BITD` 在没有可证明 palette sidecar 前继续 blocking；`Lscr` 缺源、`ediM` stream 不连续或 header/hash 不可证明时也会 blocking，不能用 metadata 摘要替代真实图像、脚本或音频转换。

Full-resource demo 还必须从 `projectorrays_full_dump_roots` 生成 `tsuinosora.projectorrays_full_dump_report.v1`。该 report 只记录 alias、file/byte counts、`chunk_fourcc_counts`、member type counts、binary signature counts、`conversion_plan`、`converted_resources` 和 converted coverage，不记录 raw dump 路径、脚本文本或素材 payload。私有 converter 要在 `local_work_root/reports/projectorrays_converted_resources.json` 写入 `tsuinosora.projectorrays_converted_resources.v1` sidecar，逐个声明 source alias、source relative path/hash、chunk FourCC、role、native asset relative path/hash、byte size 和 converter method；full dump report 会重新读取 source chunk 和 `native-assets/` 文件，校验 hash、byte size、role 和 method，并阻断 hash-only、route-only 或 raw chunk copy。`BITD`、`STXT`、`snd `、`sndH`、`sndS`、`ediM`、`XMED`、`CASt`、`Lscr`、`Lctx`、`Lnam`、`KEY_`、`mmap` 等 ProjectorRays chunk 必须有真实 converted evidence 后，才能进入 NativeVN asset sidecar、story command/source-map、package section 和 live player evidence；把 raw chunk 直接复制进 package 或只写 hash 不能算完成。

解包阶段只做读取、解包、hash、source map 和资源候选记录。不得在这一阶段直接把素材移动到 `native-assets/`，也不得只靠文件名或目录名判断素材类型。

解包报告必须记录：

- 原始 container logical id、解包项 logical id、size、hash、format probe 和 diagnostic。
- script/cast/movie/resource 引用关系。
- 受保护、不透明或无法解析资源的 coverage status。
- 是否需要进入 Asset analysis gate。
- `tsuinosora.extract_report.v1` 只能记录 `source_alias`、`output_alias`、container entry id、resource id、chunk id、offset、相对路径、size、hash、format probe、skipped reason、coverage 和 diagnostic；`tsuinosora.director_resource_map.v1` 只能记录 FourCC、offset、size、flags、hash、map version、entry size 和 diagnostic；`tsuinosora.director_cast_map.v1` 只能记录 KEY*/CAS* 关系、cast resource id、library id、child resource FourCC、size、hash、可验证的脱敏 cast metadata 字段和 diagnostic；同一个 `CASt` 被多个 `CAS*` library/slot 绑定时必须 blocking，不能作为唯一 route/source-map evidence；`tsuinosora.director_lingo_map.v1` 只能记录 Lctx/Lnam/Lscr resource id、entry id、size、hash、Lctx/Lnam entry count、text-extractable 标志和 bytecode-reader 需求，且 malformed `Lctx` table 或未终止 `Lnam` table 必须 blocking；`tsuinosora.script_source_map.v1` 只能记录 reader id/hash、source 相对路径/hash、line、route id、terminal、choice、coverage、`script_resource_id` 和 `script_payload_sha256`；source 指向含 unsupported `Lscr` bytecode 的 `director_lingo_map.json` 时，resource id/hash 必须匹配对应 Lscr resource；不得输出本地绝对路径、payload、Lingo name、正文、截图、音频或影片。

## Phase 4 Asset Analysis Gate

解包后必须先生成 `tsuinosora.asset_analysis.v1`，通过后才能写入 `local_work_root/native-assets/`。这个 gate 的目标是防止后期把素材放错位置或错误使用素材，例如把角色图放进背景目录，或把合并差分图当成单张立绘。

分析维度包括：

- 脚本引用位置、container 来源和使用时机。
- 尺寸、透明通道、visible bounding box、边缘留白、色彩分布和重复 hash；当前 helper 已在 synthetic fixture 中输出这些脱敏字段。
- 动画、atlas、切片、button hit region 或其他 metadata。
- 与 `Title.png`、`Game.png` 参考区域的布局和外观匹配结果。

分类至少包含：

| Classification | Meaning |
| --- | --- |
| `background` | 场景背景或大画幅环境图 |
| `character_sprite` | 已经可直接作为单个角色 sprite 使用的立绘 |
| `character_atlas` | 多个角色差分、表情、姿态或部件合并在同一张图中 |
| `cg` | 事件图、gallery 图或路线关键图 |
| `ui` | 通用 UI、边框、系统页装饰 |
| `text_window` | 对话框、name plate、文本窗底图 |
| `button` | 标题、系统页或菜单按钮 |
| `audio` | BGM 或非 voice 音频 |
| `voice` | 可绑定 backlog/voice replay 的语音 |
| `movie` | 影片或 movie wait 资源 |
| `font` | 字体或文本渲染资源 |
| `unknown` | 证据不足，不能进入自动重排 |

`character_atlas` 必须生成 crop/part 表、pose/expression id、anchor、layer、mouth/eye state 兼容性和 fallback。实现不能把 `character_atlas` 当作单张 `character_sprite` 使用。

低置信度或冲突分类必须进入 quarantine，并阻断 conversion report。典型阻断项包括：

- 角色图被归为 `background`。
- 背景被归为 `cg` 且缺少脚本或 gallery 证据。
- UI 边框或文本窗被当成普通背景。
- 合并差分图没有 atlas/crop 信息。
- 带透明通道的角色或 UI 图被错误展平。
- 参考截图中可见的关键 UI 或布局区域没有对应素材或命令来源。

## Phase 5 Rearrange To Native Assets

Asset analysis gate 通过后，转换器才能把解包资源重排到 `local_work_root/native-assets/`。该目录仍是本地生成产物，不进入仓库。

重排规则：

| Source kind | Native layout intent | Evidence |
| --- | --- | --- |
| script/cast text | route-scoped text table and `.astra` source map | logical id、span、hash、coverage status |
| background/cg/image | image asset set with original aspect metadata | original hash、converted hash、dimension、alpha mode、classification |
| character sprite/atlas | sprite registry、atlas crop table、pose/expression mapping | anchor、layer、crop、fallback、analysis confidence |
| UI/text window/button | system story and profile assets | region id、layout metric、reference match |
| audio/voice/sfx | AudioGraph clip registry | duration、sample rate、voice id、fence binding |
| movie | media clip registry | duration、codec class、skip policy、wait state |

转换过程必须生成 `tsuinosora.conversion_report.v1`，记录 source count、converted count、missing count、quarantine count、manual review count 和每条 route 的 coverage。当前 `Tools/TsuiNoSora/tsuinosora_tools.py` 提供公开 synthetic fixture 可测的脱敏 report builder 和 `stage3-gate` orchestrator：缺原版 source、缺解包资产、Asset analysis quarantine、native-assets rearrange 失败、cast source map 缺失、route coverage 缺口、modern fallback 缺失或路径泄露都会输出 blocking diagnostic；synthetic source/unpacked/routes/features 可以通过该 orchestrator。Asset analysis helper 已覆盖 format probe、edition fingerprint、script reference、container source、use timing、visible bbox、edge padding、颜色分布、duplicate hash、reference match、`character_atlas` crop/part 和分类冲突 quarantine；`extract-readable` 已覆盖 sidecar 复制、Director `imap`/`mmap` resource map preflight、受限 `XFIR` RIFF/RIFX exact wrapper reader、Director `KEY*`/`CAS*` cast map preflight、脱敏 `CASt` metadata kind/route/command/anchor/bounds/atlas parts/hash 读取、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight（`Lctx`/`Lnam` 只记录 entry count 和 table hash，`Lctx` 未按 32-bit entry 对齐或 `Lnam` 未按 null-terminated table 证明边界时 blocking）、RIFF/RIFX chunk 表读取、opaque/compressed `XFIR` 或尾随未验证 bytes reader-required 阻断、embedded PNG/WAV/Ogg/FLAC/MP3/MP4/script text/metadata JSON payload 抽取、可读或短 binary-header wrapped mapped `Lscr` 自动生成脱敏 source-map sidecar、protected container skipped reason 和不可读 container 阻断；`mmap` free entry 会跳过有效资源校验，只写入 `free_resource_count`，不计入 tag coverage 或 payload evidence；`imap` 存在但 `mmap` 断裂时会阻断且不退回线性扫描。Asset analysis pass 后，`stage3-gate` 会把支持的分类复制到 `local_work_root/native-assets/`，生成 `tsuinosora.native_asset_rearrange_report.v1`，并把 source/native 相对路径、classification、source hash、converted hash 和 byte size 写入 conversion report；classification 不支持、source 缺失或 hash mismatch 时 conversion 必须 blocking。`stage3-gate` 还会生成 `tsuinosora.cast_source_map_report.v1`，支持手写 `tsuinosora.cast_map.v1` 和从 Director cast map + child resource id/FourCC/extracted payload hash 派生两条公开路径，且会保留 director cast metadata 中的脱敏 route id、command id 和 atlas parts；如果 sidecar 声明 `source_hash`，必须匹配实际 extracted source asset，否则 blocking；并可以从脱敏 route graph、script source map marker、内部 generated reader sidecar 或外部 reader 产出的 `tsuinosora.script_source_map.v1` sidecar 派生 covered routes。route-bound cast member 会通过 source hash 对齐到 native-assets converted hash，并写成 patch/windows `mount_assets` evidence；sidecar 带正文、bytecode、本地路径、无效 hash、不安全 symbol、不安全 reader id/hash/output contract、未声明 route source、声明 source hash 与现有 report-relative source 文件不一致、route line 超出声明 source line_count 或 route/source hash 不一致时会 blocking；同一 route 同时来自抽出的 `.ls` 文本和 reader sidecar 时，report 优先保留 reader source-map evidence，并保留脱敏 reader id/hash evidence；存在 unsupported Lingo bytecode 且没有合规 sidecar 逐个覆盖对应 `director_lingo_map.json` source/hash 和每个 Lscr `script_resource_id`/`script_payload_sha256` 时，`tsuinosora.script_source_map_report.v1` 必须 blocking。完整 Director/Shockwave cast parser/source-map reader、真实商业 NativeVN payload 写入、真实本地 VFS/patch mount 和真实 player automation 仍是 Stage 3 后续实现项。只要 Asset analysis 仍有 quarantine，conversion report 必须保持 `blocked`。

## Phase 6 NativeVN Conversion

把 Phase 5 输出转成 AstraVN planned data。当前 `Tools/TsuiNoSora/tsuinosora_tools.py nativevn-project` 可以从已生成的脱敏 reports 和 covered routes 写出 `local_work_root/nativevn/project.yaml`、`Scripts/main.astra`、`PackageSections/*.json`、scenario refs、`native-assets/` asset sidecar 和 `tsuinosora.nativevn_package_input_report.v1`；该 report 会列出 project、story、package section、asset sidecar 和 scenario ref 的相对路径、role、`sha256` 和 byte size，不输出 story 正文或 payload。生成的 `project.yaml` 必须声明 `nativevn.asset_roots: [native-assets]`，`astra-cli cook` 会用 `astra-cook` 读取 sidecar 和 source hash，把每个 asset 写成 package section，并在 `asset.vfs_manifest.entries` 中登记 `package:/native-assets/...` URI、section id、hash 和 byte size，同时在 `asset.catalog.assets` 中登记 asset id、media kind 和 profile。生成的 `.astra` option key 和 scenario `player_input choose` 会保留 route graph/source map 中的 sanitized choice id；只有 route 没有 choice 证据时才生成 fallback choice。显式 routes 入参也会在写 story/scenario 前重新校验；不安全 `route_id`/terminal/choice、非 covered coverage、重复 choice 或冲突 route signature 会阻断，且不会写出 story 或 scenario refs。`local-gate` 会先跑 `stage3-gate`，如果没有显式 `unpacked_root`，会先执行 `extract-readable`，通过后只从 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生 routes 并写 NativeVN package input；调用方显式传入 routes 只能触发 `TSUI_LOCAL_GATE_ROUTE_EVIDENCE_REQUIRED` blocking diagnostic，不能替代商业 route coverage。`demo-slice --config` 是私有 root 入口，只接受本地 root/config 作为运行参数，输出 `tsuinosora.demo_slice_report.v1` 和 `local-gate` 派生的 NativeVN project；config 中手填 routes 会触发 `TSUI_DEMO_SLICE_ROUTE_EVIDENCE_REQUIRED`。`local_gate_report.route_count` 来自实际 NativeVN package input，包括从 route graph 或 source-map report 派生的 routes；由 stage3 reports 派生的 choices 和 route-bound `mount_assets` 也会进入 conversion report、`.astra` 和 scenario refs，不再只保留 CLI 入参。`astra-cli` 的 project-level `package_sections` 会按 target/profile 把这些脱敏 section 写入 package，写入前会移除 package release gate 禁止的 payload-like 字段，只保留 `redaction.payload: omitted`。patch/windows scenario refs 已能在 route metadata 或 cast source map 派生证据提供 `mount_assets` 时输出 alias、相对 path、role、route id 和 `sha256`，其中 role 必须是 Asset analysis 允许分类且不能是 `unknown` 或 `script`，供 Windows player 从 `--mount-root` 读取本地合法数据根。生成的 demo story 包含 `vn.commercial_baseline` 所需的 dialogue、choice、route、voice replay、movie wait、bgm、se、explicit wait 和 system UI evidence。这个 slice 只证明 package 输入、section wiring、route choice 保真、asset cook/VFS manifest/catalog 和 hash-bound mount asset 读取；真实商业文本、素材 payload、VFS reader、modern profile 和完整商业全路线自动化仍是后续 gate。

- `.astra` canonical story source。
- `CompiledStory` debug symbol、source map 和 route table。
- `VnCommandCursor` 初始位置、choice id、wait state expectation。
- Asset registry、AudioGraph、Timeline/Fence 绑定。
- classic profile 的 input map，保留左键推进和右键存档语义。
- `tsuinosora.reference_evidence`、`tsuinosora.asset_analysis`、`tsuinosora.conversion_manifest`、`tsuinosora.mount_policy` 和 scenario refs 的 package section。当前 `astra-release` 已能在 TsuiNoSora target 下阻断缺 section、schema/status 错误、路径泄露、asset quarantine、route coverage 缺口、mount target/alias 错误；`modern` profile 还会要求 `tsuinosora.modern_profile_report`，formal release profile 还会要求 `tsuinosora.manual_signoff`。当前 `astra-cli` 已能 cook project-level `package_sections` 和 NativeVN asset sidecar，并按 `targets`/`profiles` 过滤，避免 internal/patch mount policy 混写。

转换器不能把 choice payload 先写进全局 Blackboard。choice、advance、await completion 等输入应在运行时作为 trigger event payload 交给 VN step action，再由 `VnRuntimeState` 和 command cursor 推进。

当前 helper 已能在没有显式 `routes` 输入时，从 `unpacked_root` 中的脱敏 `tsuinosora.route_graph.v1` JSON 生成 `tsuinosora.route_graph_report.v1`，或从可读脚本文本 marker、内部 generated reader sidecar、外部 reader 产出的 `tsuinosora.script_source_map.v1` 脱敏 sidecar 生成 `tsuinosora.script_source_map_report.v1`，并用 covered routes 生成 scenario refs；route graph sidecar 不得带正文类字段，route id、terminal 和 choice id 必须是 safe symbol；只有 route graph 缺失时才允许尝试 script source-map fallback，坏 route graph sidecar 不能被 fallback 绕过；source-map sidecar route 必须引用同一文件中声明的 source，且 route `source_hash` 必须等于 source `sha256`，route line 必须在声明 source line_count 内，reader id/hash/output contract 必须是脱敏安全值，否则 blocking；同一 `route_id` 映射到多个 terminal/choice signature 或同一 route 内重复 choice id 时必须 blocking，不能继续生成 NativeVN story 或 scenario refs；如果 `tsuinosora.director_lingo_map.v1` 表明存在未解析 bytecode，合规 source-map sidecar 必须以匹配的 `director_lingo_map.json` source/hash 以及 Lscr `script_resource_id`/`script_payload_sha256` 证明 route coverage，否则 script source map 会保持 blocking。这些能力只覆盖公开 synthetic route metadata 和本地转换中间层 marker/sidecar；真实商业 Lingo/cast route extraction 和完整 source map 仍是后续 gate。

## Phase 7 Classic Profile

classic profile 是复刻基线。它使用 NativeVN 数据复现原版流程和感知体验：

- route 顺序、分支选择、文本、backlog 和 read-state。
- dialogue wait、choice wait、wait/movie/fence。
- 背景、CG、立绘、音效、voice、BGM、movie 的出现时机。
- 标题界面、对话界面、边框、文本窗和按钮布局。
- 左键继续、右键存档、save/load resume from wait、replay hash。

像素级和采样级差异作为诊断证据记录，不默认阻断。阻断项包括内容缺失、流程断裂、无法恢复、replay 不确定、visual reference 缺口、Asset analysis quarantine 未清或 evidence schema 不完整。

## Phase 8 Modern System Profile

modern profile 在 classic profile 之上启用 AstraVN 商业基线系统：

- title、save、load、quick save、quick load。
- backlog、voice replay、auto、skip、read-state、config。
- gallery、scene replay、movie replay、route chart。
- keyboard/gamepad/touch input map。
- system story 和 Luau policy presentation，不改写 Core 剧情状态。

系统页只能通过记录型 mutation、presentation、audio 和 timeline API 请求 effect。它不能直接修改 Core backlog、read-state、save/replay 或 route stack。

## Phase 9 Filter And Audio Enhancement

增强风格限定为修复增强：

- 缩放和 aspect handling，保留原始构图。
- 低分辨率图像滤镜、色彩/锐化 preset 和无损回退。
- 音频降噪、响度均衡、声道修复和 per-clip gain。
- 字幕和 UI 可读性增强。

所有增强都写入 `tsuinosora.modern_profile_report.v1`，记录 profile switch、preset id、输入 hash、输出 hash、可回退状态和人工听音/画面复核结果。

## Phase 10 Chinese Translation Patch

中文翻译作为额外 patch package 定义，不假设仓库可提交任何译文。patch 接口需要支持：

- text key 覆盖，不改变 command id 和 route graph。
- 字体、排版、ruby/注音、行宽和断行策略。
- 未覆盖文本 fallback 到原文。
- 翻译覆盖率、冲突、长度溢出和人工校对 signoff。

translation patch 关闭时，classic profile 的 route、hash 和 save/replay 行为必须回到未启用状态。

## Phase 11 Remake Portrait Overlay

Remake 版第一阶段只作为角色立绘 overlay 来源：

- 建立 old portrait logical id 到 remake portrait logical id 的映射。
- 为每条映射记录 anchor、scale、crop、layer、mouth/eye state 兼容性。
- 生成 alias/replacement review report。
- 支持逐角色、逐场景和全局关闭。

overlay 不能默认替换背景、CG、UI、影片或音频。任何替换失败都要回退到 original asset，并进入 modern profile report。

## Phase 12 VFS Direct-Read

VFS direct-read 是补丁式发布形态。发布包只包含 AstraVN story metadata、patch、plugin、filter preset、translation package、mount policy 和脱敏 manifest；用户在启动时选择 `original_install_root`，可选选择 `remake_install_root`。Windows player route scenario 可以用 `mount_probes` 声明 alias、相对 path 和 hash，用 route-bound `mount_assets` 声明 alias、相对 path、Asset analysis role、route id 和 hash，再由 `AstraPlayer.exe --route-scenario ... --mount-root alias=path` 在本地读取并输出 `player.patch_mount_probe` 与 `player.patch_mount_asset` check；report 不记录本地 root。Windows patch route 没有本地读取证据时，`player.patch_direct_read` 必须 blocking。Web patch direct-read 仍是后续目标，不能用浏览器静态 bundle 结果冒充本地 VFS 验收；如果 Web scenario 声明本地 mount probe/asset，Web player 必须 blocking。

VFS 插件负责：

- 读取本地原版和可选 Remake archive。
- 按 logical id 映射到 AstraEngine Asset/Media provider。
- 验证 hash 和 coverage，发现不匹配时输出诊断。
- 不绕过授权、保护或访问控制。

本地 NativeVN 转换包继续作为验证产物和回归基准，不作为公开 payload 进入仓库。公开 synthetic gate 已要求 patch Web bundle 读取 `AstraPlayer.mount_policy.json` 并输出 `player.patch_direct_read`；Windows player 已支持 `mount_probes`/route-bound `mount_assets` + `--mount-root` 的本地 probe/asset，用于证明 host 真正读取用户合法数据根中的 hash-bound 文件。完整真实本地 patch direct-read 仍要等 VFS 插件把这些 hash-bound 读取扩展到实际资源解码和 route presentation 后才能闭合。

## Phase 13 Player Automation

Stage 3 自动化需要覆盖真实玩家行为，而不只是不带窗口的 data check：

- 从 route graph 自动生成全路线 scenario。当前 helper 已能生成包含 target/profile/platform、`generated_route_id`、`player_input`、coverage、replay hash assertion，以及 patch/windows `mount_assets` 的 scenario refs；公开 synthetic real-style `demo-slice` gate 已验证生成 project 可以 cook/package/release validate，并由 bundle player route 执行。
- `tsuinosora-internal-game` 的本期范围只要求 `classic` profile demo slice 跑 package、Windows/Web bundle 和 `player.full_playable` report 校验。当前公开 regression 已覆盖该 package/bundle/report shape，私有 `.local` acceptance 入口已落地；真实 bundle 仍需要本地 config 和 live report 继续跑全路线。
- `tsuinosora-patch-game` 的 classic/modern profile、Patch-only、Runtime Patch/VFS 插件和 Web direct-read 全部是后续计划，不作为本期完成条件。
- `astra-player-core` 和 `astra-player` 已落地 `astra.player_automation_script.v1`、`astra.player_input_transcript.v1` 和 `astra.player_automation_report.v1`，只接受 Windows `sendinput.*` 或 Web `cdp.*` 输入 transcript，并要求视觉区域 hash 变化、音频 meter 和 route coverage 同时成立。
- `player.full_playable` release gate 只在显式传入 live automation report 且 package hash/profile/target 匹配时 pass；`--route-scenario`、DOM click、JS callback 或直接 `VnPlayerCommand` 会 blocking。
- 真实窗口、Chrome/Edge CDP session、系统音频 meter 和平台 host evidence 仍要由私有 acceptance run 产出。

自动化 report 只记录 state/event/presentation/player hash、route coverage、layout metric、区域 hash、diagnostic 和 pass/block 状态。

## Phase 14 Acceptance Evidence

Stage 3 `DONE` 需要自动证据通过，但正式 release profile 仍要求人工 signoff。

自动证据：

- `tsuinosora.source_inventory.v1`
- `tsuinosora.visual_reference_report.v1`
- `tsuinosora.asset_analysis.v1`
- `tsuinosora.conversion_report.v1`
- `tsuinosora.modern_profile_report.v1`
- `tsuinosora.nativevn_package_input_report.v1`
- `tsuinosora.local_gate_report.v1`
- `tsuinosora.manual_signoff.v1`，只在 formal release profile 中作为人工验收摘要进入 release gate。
- `astra.player_route_report.v1`
- scenario report、coverage summary、state/event/presentation/player hash、release report。

正式发布还需要：

- 完整通关复核。
- 听音复核。
- 画面复核。
- alias/replacement review。

任一自动阻断项未清，Stage 3 不能标为 `DONE`。人工 signoff 缺失时，Stage 3 自动化可以完成，但正式 release profile 必须保持 blocked。
