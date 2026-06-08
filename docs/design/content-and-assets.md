# Content 与 Assets 文本源设计

## 1. 目标

AstraEngine 的项目源数据必须适合人类、AI、MCP、Git diff、Review Queue、Cook 和 Release Gate。Canonical source 使用 YAML + JSON Schema；二进制资源通过 sidecar 承载语义。

## 2. 项目目录

```text
Projects/MyProject
├─ MyProject.astra.yaml
├─ Config
├─ Content
│  ├─ Characters
│  ├─ Backgrounds
│  ├─ Audio
│  ├─ Scripts
│  ├─ Graphs
│  ├─ Timelines
│  ├─ Filters
│  ├─ Lore
│  ├─ Localization
│  └─ Modernization
├─ Plugins
└─ Saved
   ├─ DerivedDataCache
   ├─ Cooked
   ├─ SaveGames
   └─ Agent
```

项目可以在 `.astra.yaml` 中显式选择可替换引擎模块 provider：

```yaml
engine_modules:
  selections:
    astra.renderer2d: project.renderer.dx11
    astra.text_layout: project.text.japanese_ruby
    astra.compat.artemis.runtime: astra.compat.artemis.default
```

`engine_modules.selections` 只表达 slot 到 provider 的选择，不复制、覆盖或重命名底层 service。
未列出的 slot 使用该 slot 的默认 provider。Release Gate 必须校验所有 slot/provider 引用有效，
并确认被选择模块满足发布权限。

## 3. 通用规则

- 所有 source object 必须有稳定 `id`。
- 列表项使用稳定 ID，不靠数组位置表达语义。
- 长文本使用 YAML block scalar。
- 字段标记 `ai_editable`、`tool_generated`、`read_only`、`requires_review`。
- Cooked、DerivedDataCache、package manifest 不是 AI/MCP 编辑源。

## 3.1 Content Browser 工作流

Content Browser 面向创作者，必须支持：

- 导入：通过 import preset 生成 source asset、sidecar、tags、license 和 review 状态。
- 批量操作：rename、move、retag、license update、metadata patch，必须更新引用或给出 broken reference diagnostics。
- 依赖查看：显示 hard/soft reference、script/graph/timeline 引用、cook dependency。
- 引用修复：missing asset 可选择重新定位、替换为 virtual asset、删除引用或创建 placeholder。
- 迁移：跨项目复制 native asset 时保留 provenance、license、review 和 dependency closure。
- AI draft import：draft 必须进入 Review Queue，接受后才成为 `native:/` asset。

资产状态机：

```text
External File -> Source Asset -> Registered Asset -> Cooked Asset -> Packaged Asset
AI Draft -> Review -> Accepted -> Source Asset
Generated/Enhanced Draft -> Review -> Accepted -> Source Asset
Rejected Draft -> Audit Only
```

Creator-facing import presets：

- `sprite`：角色立绘、表情、layer/order 默认值。
- `background`：背景图、scene usage、filter defaults。
- `voice`：角色语音、line binding、volume bus。
- `music`：BGM、loop point、fade defaults。
- `font`：fallback set、locale、atlas policy。
- `filter_profile`：background/character/ui/text/final target presets。
- `timeline`：track template、camera/audio/dialogue event defaults。

Project template descriptor 示例：

```yaml
id: astra.template.vn.standard
display_name: Standard AstraVN
runtime_profile: astra.vn.runtime
engine_modules:
  astra.renderer2d: astra.renderer2d.default
  astra.text_layout: astra.text.default
  astra.audio: astra.audio.default
content_seed:
  characters: [Alice]
  backgrounds: [Room]
  scripts: [Scripts/main.astra]
wizard:
  required_fields: [project_name, locale, target_platform]
  optional_steps: [sample_assets, ai_policy, package_profile]
acceptance:
  - astra validate ${project}
  - astra cook ${project} --config Debug
  - astra run ${project}/Saved/Cooked --headless-smoke
```

Asset import preset 示例：

```yaml
id: astra.import.sprite.character
source_extensions: [.png, .webp]
asset_type: image
sidecar_defaults:
  tags: [character]
  origin: HumanAuthored
  review:
    status: accepted
cook_defaults:
  texture_preset: sprite
  atlas: characters
diagnostics:
  missing_alpha: warning
  oversized_texture: blocking_for_release
```

Review queue item 示例：

```yaml
id: review:/asset/2026-06-05/alice_sprite
kind: asset_import
state: pending
source_ref: Saved/Agent/Drafts/alice_sprite.png
target_ref: native:/Characters/Alice/Normal
diagnostics: []
actions:
  accept: import_asset
  reject: audit_only
  revise: create_new_draft
```

## 4. AssetId

AssetId 支持：

```text
native:/Characters/Alice/Normal
foreign-bgi:/data/fg.arc#alice_idle
foreign-krkr:/fgimage/alice_happy
virtual:/current/character/alice
```

`native:/` 只表示 Astra 项目自有资产。外部原游戏资产必须使用 `foreign-*`，不得伪装为 native。

## 5. Sidecar

二进制资源使用同名 `.asset.yaml`：

```yaml
id: native:/Characters/Alice/Normal
type: image
source_path: Characters/alice_normal.png
display_name: Alice Normal
tags: [character, alice]
origin: HumanAuthored
license:
  owner: project
  usage: internal
cook:
  texture_preset: sprite
```

AssetRegistry 由 sidecar 扫描生成，不作为人工或 AI 编辑源。

AI-generated asset sidecar 目标态示例：

```yaml
id: native:/Backgrounds/RainyStreet/Draft01
type: image
source_path: Backgrounds/rainy_street_draft01.png
display_name: Rainy Street Draft 01
tags: [background, rainy, street]
origin: AIGenerated
requires_review: true
review:
  status: pending
  review_item: review:/ai/2026-06-05/rainy_street_draft01
ai_generation:
  provider: astra.ai.provider.example
  session_hash: sha256:...
  prompt_hash: sha256:...
  context_hash: sha256:...
  output_hash: sha256:...
  source_draft: Saved/Agent/Drafts/rainy_street_draft01
license:
  owner: project
  usage: internal
  source: ai_generated
cook:
  texture_preset: background
```

AI 生成资产成为 `native:/` 正式资产前，必须有稳定 `id`、sidecar、license、review 状态和 Generation Audit 链接。被拒绝或取消的 draft 不进入 AssetRegistry，不参与 Cook；被接受后才复制或移动到 Content 并生成正式 sidecar。`origin: AIGenerated` 不表示自动可发布，Release Gate 仍按发布模式检查。

## 6. FilterProfile

FilterProfile 是文本源资产：

```yaml
id: native:/Filters/legacy_character_clean
type: filter_profile
passes:
  - filter: denoise
    target: character
    strength: 0.25
  - filter: anime_line_enhance
    target: character
    strength: 0.5
```

目标 target 至少包含 `background`、`character`、`ui`、`text`、`final`。

## 7. 外部资产元数据

```yaml
id: foreign-bgi:/data/cg.arc#opening
type: image
external_source:
  root: compatibility.external_project_root
  package: data/cg.arc
  member: opening
usage: background_cg
cook:
  allow_copy: false
```

Mount-only 项目默认不复制外部原始资产。现代化替换必须引用授权的 `native:/` 资产。

## 8. Release Gate

Release Gate 检查 YAML、schema、重复 ID、缺失 sidecar、broken dependency、AI-editable 边界、foreign asset root、mount-only copy policy、FilterProfile target 和 plugin descriptor。

Creator acceptance：

- 创作者可导入角色、背景、语音、音乐、字体和 filter profile，并在 Content Browser 中看到依赖与诊断。
- AI 生成或增强的 draft 没有 accepted review 前不能进入 Cook。
- 批量移动或重命名资产后，引用要么自动修复，要么产生可点击 diagnostics。
