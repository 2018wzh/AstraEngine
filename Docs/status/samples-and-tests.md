# Samples And Test Matrix

## Hard Samples

| Sample | Purpose | Required scenarios |
| --- | --- | --- |
| NativeVN Minimal | EngineCore + AstraVN smoke | boot、command cursor、dialogue wait、choice payload、save/load resume from wait、replay、package |
| EngineCore Native Smoke | Stage 1 EngineCore implemented sample | [native_smoke.yaml](../../scenarios/native_smoke.yaml)、dialogue event、choice event、save/load、replay hash |
| NativeVN Commercial Baseline | 商业 VN 基线系统 | command cursor、backlog、auto、skip、read-state、config、gallery、replay、route chart、voice replay、movie、transition、movie/voice/timeline await |
| AstraVN Script Policy | 机制/策略分离样例 | [script sample](../samples/astra-vn-script/README.md)、Luau policy、Timeline/Fence、choice selected payload、localization、system stories |
| AstraVN Advanced Presentation | 旗舰演出 opt-in profile | [advanced sample](../samples/astra-vn-advanced/README.md)、多层 stage、camera、video layer、shader/filter、voice fence、timeline join/cancel、system UI、save/load resume from wait、`vn.advanced_presentation` |
| TsuiNoSora Local Port | 真实项目压力样例 | [modernization sample](../samples/tsuinosora-modernization/README.md)、classic/modern profile、full route、media coverage、release report、manual signoff |

## AstraEMU Family Samples

每个 family 使用用户本地合法数据，报告只提交 hash 和脱敏 metadata。v1 可用 family 是 Artemis；其他 family 输出 alpha probe report。实现顺序：Artemis、KrKr、BGI、SoftPAL、FVP、Siglus。

## Scenario Format

下面是 Stage 3 VN scenario 当前已落地的 slice。`Examples/NativeVN/project.yaml` 和 `Examples/AdvancedVN/project.yaml` 会通过真实 `astra cook` 和 `astra package build` 产出 package，runner 再从 package 读取 `vn.compiled_story`，通过 player 层 action 推进 dialogue、choice、system page、save/load、voice replay、`complete_wait`、advanced check assertion 和 replay hash。`astra package bundle` 已能从同一个 `.astrapkg` 生成 Windows/Web standalone bundle，并把 `scenario.refs` 中的公开 scenario 复制进 bundle；Windows entrypoint 无参数启动会输出 `astra.player_launch_report.v1`，`AstraPlayer.exe --route-scenario` 会从 bundle 内读取 config、package、scenario refs 和脱敏 `AstraPlayer.mount_policy.json` 后输出 `astra.player_route_report.v1`；Web bundle 额外写入 route model 和 scenario JSON，由真实 headless browser host 读取 package hash、route model、scenario 和 mount policy 后在 DOM 输出同 schema 的 route report。Scenario schema 已支持 target/profile/platform、generated route id、mount alias、`mount_probes`、route-bound `mount_assets`、`player_input`、coverage/hash/check assertion，以及基于 `tsuinosora.reference_evidence` 的 visual reference assertion；report 只记录 hash、coverage、diagnostic 和 region id，不输出新商业截图。`tsuinosora.visual_reference_report.v1` 会校验默认 `Title.png`/`Game.png` 固定尺寸和 hash，缺文件、PNG 不可读或 evidence mismatch 会 blocking。`mount_probes` 只能声明 alias、相对 path 和 sha256；`mount_assets` 只能声明 alias、相对 path、role、route id 和 sha256；实际本地 root 只通过 Windows player 的 `--mount-root alias=path` 参数传入，不进入 scenario、package 或 report。`astra-vn` 侧还覆盖 call/return stack、duplicate id / missing target / reachability compiler diagnostic、Runtime save container 的 `vn.runtime_state`/`vn.policy_state`、rich backlog、`VnReplayUiState` replay UI snapshot、SystemStoryManifest、VnPolicyBundleManifest、VnExtensionManifest、VnStandardCommandManifest、VnPresentationProviderManifest、VnCommercialBaselineManifest、VnAdvancedPresentationManifest、StageModel/VideoLayer/AudioCommand/Timeline lifecycle、VnWaitState、Graph/Timeline metadata、Luau mutation trace、rollback scope/playback、command/query/trace capability 和 snapshot value policy；TsuiNoSora package section release gate 已覆盖参考证据、Asset analysis、conversion manifest、mount policy、modern profile report 和 formal release manual signoff 的脱敏阻断。TsuiNoSora helper 已记录 `tsuinosora.extract_report.v1` direct-readable sidecar 复制、Director `imap`/`mmap` resource map preflight 和 `free_resource_count`、受限 `XFIR` RIFF/RIFX exact wrapper reader、opaque/compressed `XFIR` 与尾随未验证 bytes reader-required blocking、Director `KEY*`/`CAS*` cast map preflight、Director `Lctx`/`Lnam`/`Lscr` Lingo map preflight、`Lnam` entry count/table hash、从 Director cast map 与 child resource id/FourCC/extracted payload hash 派生 `tsuinosora.cast_source_map_report.v1`、受限 RIFF/RIFX chunk 表读取、embedded image/audio/movie/script text/metadata JSON payload 抽取、手写 `tsuinosora.cast_map.v1` member/source/container entry/hash 映射、cast sidecar source hash mismatch blocking、`tsuinosora.route_graph_report.v1` covered route 派生与 route graph payload/unsafe symbol blocking、`tsuinosora.script_source_map_report.v1` route marker/source line 派生、可读或短 binary-header wrapped mapped `Lscr` 自动生成 `director_lingo_source_map.json`、`tsuinosora.script_source_map.v1` 脱敏 reader sidecar 派生、reader id/hash/output contract evidence、payload/path/hash/symbol blocking、reader sidecar declared source hash mismatch blocking、route line out-of-range blocking、sidecar route source/hash mismatch blocking、合规 sidecar 覆盖 unsupported Lingo bytecode、diagnostic 去重和 unsupported Lingo bytecode 阻断、重复 route 优先保留 reader source-map evidence，以及未解析或不可读 Director/Shockwave container 阻断；Asset analysis helper 已记录 script reference、container source、use timing、visible bbox、edge padding、颜色分布、duplicate hash、reference match 和 classification conflict quarantine，`local-gate` 已能把 stage3 gate 和 NativeVN package input 写入串成单个本地入口，`nativevn_package_input_report` 会列出 project/story/section/scenario 文件的相对路径、role、hash 和 byte size。`package_sections` 已能按 target/profile 把这些脱敏 section 写入 package，公开 synthetic TsuiNoSora project 已跑通 internal classic/modern headless、Windows player、Web player 以及 patch classic/modern headless、Windows player、Web player 全路线，并对 patch target 输出 `player.mount_policy_hash`、Windows `player.patch_mount_probe`、`player.patch_mount_asset` 和 `player.patch_direct_read`，mount policy 文件被篡改时会 blocking。未实现的 VN action/assertion 仍必须输出 blocking diagnostic。

这些 bundle 和 route report 只证明 package、scenario refs、route model、mount policy 与脱敏 route evidence 可以被 host 读取；它们不满足 `player.full_playable`。当前 `astra-player-core`/`astra-player` 已提供 automation script/transcript/report 校验，release gate 只接受匹配 package hash/profile/target 的 live report。Stage 3 live player host acceptance 仍需要 Windows `SendInput`、Web CDP input、视觉区域 hash 变化、音频 meter 和平台 host evidence 同 run 产出，且不得使用 `VnPlayerCommand`、DOM click、JS callback、API 可用性 smoke 或 `--dump-dom` route runner 冒充玩家输入。

补充：Director Lingo preflight 也记录 `Lctx` entry count/table hash；`Lctx` 和 `Lnam` 都只保留结构 hash 和计数。Malformed `Lctx` table 或未终止 `Lnam` table 会 blocking。

补充：Director cast preflight 会阻断重复 `CASt` binding；同一个 `CASt` 不能被多个 `CAS*` library/slot 静默折叠成一条 member evidence。

补充：Cast source-map report 会阻断手写 `tsuinosora.cast_map.v1` 与外部 `tsuinosora.director_cast_map.v1` sidecar 中的 payload/正文/bytecode 字段，避免素材映射证据携带商业内容。

补充：Director cast map report 会读取显式 `tsuinosora.director_cast_member_metadata.v1` 的脱敏 kind、route id、command id、anchor、bounds 和 metadata hash，并把 kind/route/command 传递到 cast source-map report；普通 `CASt` payload 仍只记录 hash。

补充：Director cast metadata 的 anchor/bounds 也进入测试覆盖；anchor 非数值或 bounds 负尺寸会 blocking。

补充：Director cast metadata 的 `character_atlas` parts 也进入测试覆盖；缺 parts 会 blocking，合规 parts 会传递到 cast source-map report。

补充：Route graph 和 script source-map report 会阻断重复 `route_id` 冲突；同一 `route_id` 不能指向多个 terminal/choice signature 后继续写入 NativeVN package input。

补充：Route graph 和 script source-map report 也会阻断同一 route 内重复 choice id，避免重复 choice 进入 `.astra` option key 或 scenario `player_input choose`。

补充：`stage3-gate` 的 script source-map fallback 只用于 route graph 缺失；如果 route graph sidecar 存在但带 payload、unsafe symbol、coverage 缺口或 duplicate route/choice，Stage 3 report 仍会 blocking。

补充：`tsuinosora.script_source_map.v1` 覆盖 unsupported `Lscr` bytecode 时，route 必须声明匹配的 `script_resource_id` 和 `script_payload_sha256`；缺失、未知 resource id 或 hash mismatch 会输出 blocking diagnostic，report 仍只记录 resource id、hash 和错误代码。

TsuiNoSora helper 还会在 Asset analysis pass 后写入 `local_work_root/native-assets/`，生成 `tsuinosora.native_asset_rearrange_report.v1`，并把 source/native 相对路径、classification、source hash、converted hash、byte size、converted asset count 和 missing asset count 写进 conversion report；rearrange blocked 时 conversion blocked。NativeVN package input 会保留 route graph/source map 的 sanitized choice id，写入 `.astra` option key 和 scenario `player_input choose`，多 choice route 不会被压缩成单个 synthetic choice；显式 route 输入也会先校验 symbol、coverage、duplicate choice 和 route signature，失败时不写 story/scenario refs。`local-gate` 已收紧为只接受 `stage3-gate` 从 route graph 或 script source-map report 派生出的 routes；显式 routes 不能作为商业 coverage evidence。`stage3-gate` 可以把 route-bound cast member 与 native-assets hash evidence 合成 patch/windows `mount_assets`，并在 `local-gate` 从 report 派生 routes 时继续保留这些 mount evidence。

Windows patch route 如果没有 `mount_probes` 或 `mount_assets` 本地读取证据，`player.patch_direct_read` 必须 blocking。`mount_assets.role` 必须使用 Asset analysis 允许分类，且不能用 `unknown` 或 `script` 伪造本地素材证据。

```yaml
schema: astra.scenario.v1
package: target/nativevn.astrapkg
target: nativevn-game
profile: classic
platform: windows
generated_route_id: route.library
mount_aliases:
  original: original_install_root
mount_probes:
  - alias: original
    path: probe/manifest.json
    sha256: sha256:...
mount_assets:
  - alias: original
    path: native-assets/backgrounds/opening.png
    role: background
    route_id: route.library
    sha256: sha256:...
seed: 42
actions:
  - launch: {}
  - player_input:
      kind: advance
  - player_input:
      kind: choose
      value: choice.library
  - player_input:
      kind: advance
  - player_input:
      kind: replay_voice
      value: voice.hero.0002
  - player_input:
      kind: complete_wait
      value: movie.opening.end
  - player_input:
      kind: open_system
      value: route_chart
  - player_input:
      kind: save
      slot: slot.auto
  - player_input:
      kind: load
      slot: slot.auto
  - player_input:
      kind: set_auto
      value: "true"
  - player_input:
      kind: set_skip
      value: read
  - player_input:
      kind: set_config
      key: text_speed
      value: instant
  - player_input:
      kind: unlock_gallery
      value: cg.opening
  - replay_from_start: {}
assertions:
  - coverage:
      routes: [ending.good]
      backlog_keys: [library.followup]
      read_state: [line.library]
      voice_replay: [voice.hero.0002]
  - system_state:
      auto_enabled: true
      skip_mode: read
      config:
        text_speed: instant
      gallery_unlocks: [cg.opening]
  - hash:
      state: hash128:...
      event: hash128:...
      presentation: hash128:...
  - visual_reference:
      id: title
      hash: sha256:...
      regions: [menu]
  - replay_hash_match: true
  - no_blocking_diagnostics: true
```
