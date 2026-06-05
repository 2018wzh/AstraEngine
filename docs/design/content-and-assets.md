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

## 3. 通用规则

- 所有 source object 必须有稳定 `id`。
- 列表项使用稳定 ID，不靠数组位置表达语义。
- 长文本使用 YAML block scalar。
- 字段标记 `ai_editable`、`tool_generated`、`read_only`、`requires_review`。
- Cooked、DerivedDataCache、package manifest 不是 AI/MCP 编辑源。

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
