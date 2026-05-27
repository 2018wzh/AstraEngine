# Content 与 Assets 文本源设计

## 1. 目标

AstraEngine 的源项目内容必须对人类、AI、MCP、Git diff、Review Queue 和 Cook Pipeline 友好。

核心目标：

- 源数据文本优先。
- 稳定 ID 优先。
- Schema 校验优先。
- 二进制资源语义通过 sidecar 描述。
- Cooked content 和 DerivedDataCache 不作为 canonical source。

## 2. Canonical Source Format

规范源格式：

- YAML。
- JSON Schema 校验 YAML 解析后的数据节点。

规则：

- 文件允许注释。
- 长文本字段使用 YAML block scalar。
- 列表项使用稳定 ID，避免靠数组位置表达语义。
- 字段分为 AI-editable、tool-generated、read-only。
- 所有 source object 必须有稳定 `id`。

示例：

```yaml
id: character.alice
name: Alice
summary: |
  A quiet student who hides her anxiety behind precise language.
ai_editable:
  - summary
  - voice_style
voice_style: |
  Polite, restrained, with short pauses before emotionally difficult lines.
```

## 3. Project Layout

```text
Projects/MyVisualNovel
├── MyVisualNovel.vnproj.yaml
├── Config
│   ├── DefaultGame.yaml
│   ├── DefaultEditor.yaml
│   ├── DefaultAI.yaml
│   ├── DefaultInput.yaml
│   └── DefaultPackaging.yaml
├── Content
│   ├── Characters
│   │   └── alice.character.yaml
│   ├── Backgrounds
│   │   ├── rooftop.png
│   │   └── rooftop.png.asset.yaml
│   ├── Audio
│   │   ├── quiet_wind.ogg
│   │   └── quiet_wind.ogg.asset.yaml
│   ├── Scripts
│   │   └── chapter_01.astra
│   ├── StoryGraphs
│   │   └── chapter_01.story.yaml
│   ├── Lore
│   │   └── memory_fragment_rule.lore.yaml
│   ├── Localization
│   │   └── zh-CN.loc.yaml
│   └── Tests
├── Schemas
│   ├── asset.schema.json
│   ├── plugin.schema.json
│   ├── character.schema.json
│   ├── lore.schema.json
│   ├── story.schema.json
│   ├── localization.schema.json
│   └── review.schema.json
├── Plugins
│   └── ExamplePlugin
│       └── ExamplePlugin.plugin.yaml
└── Saved
    ├── AI
    │   ├── ReviewQueue
    │   │   └── patch_0001.review.yaml
    │   └── Provenance
    │       └── audit_0001.audit.yaml
    └── MCP
        └── OperationLog
            └── mcp_0001.mcp-operation.yaml
```

## 4. Asset Sidecar

Images, audio, fonts, Live2D, Spine and other binary assets do not carry canonical semantic metadata. Each binary source asset should have a sibling `.asset.yaml`.

Example:

```text
Content/Characters/alice_normal.png
Content/Characters/alice_normal.png.asset.yaml
```

Sidecar example:

```yaml
id: native:/Characters/Alice/Normal
type: image
source_path: Characters/alice_normal.png
display_name: Alice Normal
tags:
  - character
  - alice
  - standing
dependencies: []
origin: HumanAuthored
description: |
  Alice standing sprite with a neutral expression.
ai_notes: |
  Use this sprite for restrained or default dialogue.
license:
  owner: project
  usage: internal
cook:
  texture_preset: sprite
  generate_thumbnail: true
```

Runtime config should live in project YAML config:

```yaml
runtime:
  entry_script: Content/Scripts/main.astra
compatibility:
  enabled: false
```

Mount-only compatibility projects use external references:

```yaml
compatibility:
  enabled: true
  modules:
    - astra.plugin.director_compatibility
  external_project_root: "E:/Archive/ExampleGame"
  mount_only: true
  allow_asset_copy: false
```

Required sidecar fields:

- `id`
- `type`
- `source_path`
- `origin`

Suggested first-stage `type` values:

- `image`
- `audio`
- `font`
- `live2d`
- `spine`
- `video`
- `data`

Recommended fields:

- `display_name`
- `tags`
- `dependencies`
- `description`
- `ai_notes`
- `license`
- `cook`

## 5. Generated AssetRegistry

AssetRegistry is generated from sidecars.

```text
Sidecar .asset.yaml
  -> Validate YAML
  -> Validate JSON Schema
  -> Check duplicate AssetId
  -> Resolve dependencies
  -> Generate editor index
  -> Generate cooked runtime registry
```

Rules:

- Humans and AI edit sidecars, not generated registry files.
- Generated registry may be JSON, binary, SQLite, or another optimized format.
- Registry generation fails on duplicate IDs, missing source files, missing required sidecars, invalid dependencies, or schema mismatch.

## 5.1 External Asset Metadata

Compatibility modules may index assets that remain in a user-owned external game directory. These assets are not Astra canonical source assets and must not be copied into cooked output by default.

External metadata records references and authoring notes only:

```yaml
id: foreign-director:/DATA/CASTS/CHARS.cxt#member=alice_idle
type: image
external_source:
  root: compatibility.external_project_root
  package: DATA/CASTS/CHARS.cxt
  member: alice_idle
usage: character_sprite
tags:
  - character
  - alice
modernization_notes: |
  Use Astra dialogue UI, scaling, save system, and localization overlays.
cook:
  allow_copy: false
```

Rules:

- `foreign-*` AssetId values identify external assets.
- `native:/` remains reserved for Astra-owned source assets.
- External metadata may live in project text sources, but the referenced binary remains outside the project.
- Cook/package fails if external binaries would be copied while `allow_copy` is false.
- Release Gate reports missing external roots, broken package/member references, and invalid external metadata.

## 6. Authoring Text Sources

### 6.1 Character

```yaml
id: character.alice
display_name: Alice
asset_refs:
  default_sprite: native:/Characters/Alice/Normal
voice_style: |
  Quiet, exact, hesitant when emotional.
ai_editable:
  - display_name
  - voice_style
```

### 6.2 Lore

```yaml
id: lore.memory_fragment_rule
locked: true
ai_can_reference: true
ai_can_modify: false
content: |
  Memory fragments cannot be read intentionally. They surface only when emotional contact is strong.
```

### 6.3 Story Graph

```yaml
id: story.chapter_01
entry: scene.school_rooftop
nodes:
  - id: scene.school_rooftop
    type: scene
    script: Scripts/chapter_01.astra
  - id: choice.apology
    type: choice
    options:
      - id: apologize
        text_key: chapter_01.choice.apologize
        target: scene.apologize_branch
```

### 6.4 Localization

```yaml
locale: zh-CN
entries:
  - key: chapter_01.alice.line_0120
    speaker: character.alice
    text: |
      你来了。
```

### 6.5 Review Queue

```yaml
id: review.patch_0001
type: script_dialogue_replace
target: Content/Scripts/chapter_01.astra#line_120
status: pending
origin: AISuggested
before: |
  alice "你来了。"
after: |
  alice "我还以为你不会来了。"
reason: |
  增强角色的失落感，与上一段等待场景呼应。
```

### 6.6 Plugin Descriptor

Dynamic module descriptors are also text source data:

```yaml
id: astra.plugin.live2d
display_name: Live2D
version: 0.1.0
astra_api: ">=0.1 <0.2"
modules:
  - id: live2d.runtime
    type: runtime
    entrypoint: Bin/win64/Live2DRuntime.dll
    load_phase: runtime_startup
    capabilities:
      - service_extension
      - cook_processor
    permissions:
      runtime:
        packaged: true
```

Rules:

- Plugin descriptors use stable IDs and schema validation.
- Capabilities and permissions must match registered ExtensionRegistry entries.
- Editor, Developer and MCP debug modules are excluded from packaged runtime by default.

## 7. AI-Friendly Rules

- Prefer stable IDs over relative array positions.
- Prefer explicit references over implicit path conventions.
- Prefer block scalars for dialogue, lore, notes, and prompt text.
- Keep generated fields in separate generated outputs where possible.
- Mark fields that AI may edit.
- Keep binary metadata in sidecar files near the binary source.
- Keep Cooked, DerivedDataCache, and package output out of AI editing scope.

## 8. Release Gate Checks

Release Gate must validate:

- YAML parse success.
- JSON Schema conformance.
- Stable ID presence.
- Duplicate ID absence.
- Required asset sidecars exist.
- Sidecar `source_path` exists.
- Asset dependencies resolve.
- External asset roots and package/member refs resolve when a compatibility module is enabled.
- External original assets are not copied into cooked output unless explicitly authorized.
- AI-editable and read-only field boundaries are respected.
- Generated registry is up to date with sidecars.
- PluginDescriptor schema is valid, dynamic module dependencies resolve, and packaged runtime module rules are respected.
