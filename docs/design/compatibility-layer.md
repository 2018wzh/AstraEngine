# 旧 VN 引擎模拟器与现代化设计

## 1. 目标

Compatibility Layer 的目标是支持现有 VN 引擎项目或游戏包的运行、诊断、调试和现代化。它不是 Import 工具，不要求把外部脚本转换为 Astra DSL，也不默认复制外部原始资产。

Compatibility Layer 是 **Expansion Track**。它排在 native runtime production parity 之后：
必须消费稳定的 Runtime、Asset、Media、Script、Save 和 FilterGraph API，不能成为 Core、
Runtime、Asset 或 Media 达到 UE-class 2D runtime 完备度的前置条件。

支持方向：

- BGI、Kirikiri、Ren'Py、NScripter、Director、TyranoScript 等兼容模块。
- 旧 VM / opcode / timeline / score 模拟。
- 资源包读取、外部资产引用、变量和脚本调试。
- 字体替换、UI 覆盖、滤镜增强、高清资源覆盖、缩放策略。

## 2. 核心结构

```text
Compat Plugin
├─ ForeignProjectProbe
├─ PackageReader / VFS Mount
├─ LegacyAssetResolver
├─ LegacyScriptRuntime / VM
├─ OpcodeDecoder / TimelineAdapter
├─ LegacyApiMapper
├─ ModernizationProfileProvider
├─ SaveExtensionStateProvider
└─ CompatibilityInspector
```

运行链路：

```text
External Game Data
  -> VFS / PackageReader
  -> Legacy VM / ScriptRuntime
  -> Legacy API Mapper
  -> RuntimeEvent / PresentationCommand
  -> Actor / StateMachine / Media / FilterGraph
```

## 3. Legacy Script Runtime

旧引擎兼容应优先实现同级脚本运行时，而不是强制反编译：

```cpp
class ICompatRuntime {
public:
    virtual bool probe(const ForeignProjectDesc& project) = 0;
    virtual bool load(const ForeignProjectMount& mount) = 0;
    virtual void start(std::string_view entry) = 0;
    virtual void update(double dt) = 0;
    virtual void send_event(const RuntimeEvent& event) = 0;
    virtual CompatRuntimeState save_state() = 0;
    virtual void load_state(const CompatRuntimeState& state) = 0;
};
```

VM 可以维护 PC、栈、变量、调用栈、score frame、timeline cursor 等私有状态，但必须通过 Save extension state 进入统一存档。

CompatRuntimeProvider descriptor：

```yaml
provider_id: astra.compat.bgi.runtime
contract: CompatRuntimeProvider
module_id: astra.compat.bgi
slot_id: astra.compat.bgi.runtime
supported_projects:
  - probe: bgi.system_ini
  - probe: bgi.arc_layout
capabilities:
  package_reader: true
  vm_debug: true
  save_extension_state: true
  modernization_profile: true
permissions:
  foreign_mount_read: true
  project_write: false
  packaged: false
release_gate:
  expansion_only: true
  mount_only_default: true
```

运行时集成规则：

- Compat runtime 作为 `IScriptRuntimeProvider` 或 `CompatRuntimeProvider` 进入 ScriptRuntimeHost。
- VM step 由 RuntimeScheduler 调度，不直接拥有主循环。
- VM 输出通过 LegacyApiMapper 转为 RuntimeEvent 或 PresentationCommand。
- VM 私有状态只能作为 Save extension state 保存，不写入 native Actor/Component schema。
- Compat 模块不能要求 Core、Runtime、Asset、Media 反向暴露旧引擎专用 API。

## 3.1 Package Reader / Foreign Project Probe

ForeignProjectProbe 输入：

- foreign root path。
- release profile。
- allowed engine families。
- user-declared license/mount policy。

输出：

- detected engine family/version。
- package table。
- script entry。
- asset roots。
- unsupported feature diagnostics。

PackageReader contract：

```cpp
class ILegacyPackageReader {
public:
    virtual ProbeResult probe(const ForeignProjectRoot& root) = 0;
    virtual PackageMount mount(const ForeignProjectRoot& root, MountPolicy policy) = 0;
    virtual ReadResult read_member(ForeignAssetId id, ByteSpan* out) = 0;
    virtual PackageIndex index() const = 0;
};
```

错误策略：

- encrypted/protected package：不绕过保护，输出 unsupported diagnostics。
- ambiguous engine version：要求用户选择或提供 profile。
- missing external root：Compatibility Inspector 显示 remount action。
- illegal copy request：Release Gate blocking diagnostic。

## 3.2 Anonymous Artemis 2025 VN Case Study

匿名 Artemis 2025 VN 案例用于约束兼容层设计。文档只记录格式、目录和数量级，不记录本地路径或具体作品名。

安装态布局：

```text
Game Root
├─ *.exe
├─ *.dll
├─ *.pfs
├─ *.pfs.000
├─ *.pfs.721
├─ movie/*.dat
├─ *.ttf
├─ readme.txt
└─ *.bat
```

设计影响：

- 真实交付形态需要 Artemis PackageReader 识别 `.pfs` 分卷和 movie data。
- v1 可以先支持 unpacked directory；`.pfs` reader 是后续阶段。
- encrypted/protected package 只产生 unsupported diagnostics，不实现破解、解密或绕过保护。
- 安装态 probe 只能建立 package index、entry script candidate 和 mount policy，不能复制原始资产。

解包态布局：

```text
Unpacked Root
├─ font
├─ image
├─ pc
├─ script
├─ sound
├─ system
└─ system.ini
```

观察到的资源规模：

- approximately `10099 .ogg`。
- approximately `1216 .png`。
- approximately `313 .sli`。
- approximately `78 .ast`。
- approximately `54 .lua`。
- `3 .asb`、`2 .iet`、font files、table/config files。

启动链路：

```text
system.ini
  -> system/first.iet
  -> system/init.lua
  -> system/script.asb
  -> script/*.ast
```

Artemis 兼容不是单一 `.ast` parser。真实运行需要同时处理：

- `.iet` text tag script。
- `.asb` binary tag script。
- `.ast` Lua-table story block/index data。
- system Lua modules。
- `e:*` host API。
- tag executor、resource resolver、LegacyApiMapper。

## 3.3 Artemis v1 Implementation Route

Artemis v1 目标是可诊断、可逐步运行的 unpacked-directory 兼容原型，不是完整生产兼容。

优先实现：

- Probe：识别 `system.ini`、`system/first.iet`、`system/*.asb`、`script/*.ast` 和 media roots。
- VFS：以 mount-only policy 注册 `foreign-artemis:/`，保留 Artemis magic path 到 foreign asset 的映射。
- Script index：解析或索引 `.iet`、`.asb`、`.ast`、`.ipt`、`.sli`、`.tbl`。
- Lua host：提供 Artemis `e` 对象的最小 API surface。
- Tag coverage：优先覆盖 `bg`、`fg`、`text`、`msg`、`vo`、`se`、`bgm`、`select`、`excall`、`wait`、`extrans`、`msgoff`、`quake`、`ruby`、`eval`、`movie`。
- Inspector：输出 unsupported tag/API/asset coverage、script location、fallback 和 severity。

`foreign-artemis:/` examples：

```text
foreign-artemis:/image/bg/bg001a.png
foreign-artemis:/image/fg/kot/z1/kot_z1a0000.png
foreign-artemis:/sound/bgm/bgm003.ogg
foreign-artemis:/sound/vo/asu/fem_asu_00002.ogg
```

Artemis host API v1 coverage：

- file: `include`、`file`、`isFileExists`。
- script: `tag`、`enqueueTag`、`setScriptStatus`、`getScriptStack`。
- variable: `var`、deterministic random、time through Runtime services。
- input: key/mouse/touch query mapped through Platform input snapshot。
- surface/cache: `bindSurface*`、`unbindSurface`、`clearSurfaceLoadQueue` may begin as tracked no-op with diagnostics, then graduate to Media-backed behavior。

## 4. API Mapper

Legacy API Mapper 把旧引擎调用映射为 Astra 事件或 Presentation Command：

- 图像显示 -> Character/Background/Layer event。
- 文本输出 -> Dialogue event。
- 音频 -> Audio cue event。
- 选择 -> Choice event。
- 转场 -> Timeline 或 FilterProfile event。
- 系统变量 -> Blackboard 或 compat state。

Mapper 不直接调用 Renderer2D 或 AudioCore native handle。

Mapper rule 示例：

```yaml
id: bgi.show_character
legacy_call:
  opcode: show
  args: [layer, asset, x, y, transition]
astra_output:
  event: astra.vn.character.show_requested
  payload:
    actor: map.character(layer)
    asset: map.asset(asset)
    transform:
      position: [x, y]
    transition: map.transition(transition)
fallback:
  missing_asset: placeholder_character
diagnostics:
  unsupported_transition: ASTRA_COMPAT_BGI_010
```

Mapping rules：

- 所有 legacy asset reference 必须映射为 `foreign-*` 或授权 `native:/` modernization replacement。
- 文本输出优先进入 Dialogue event 和 TextLayout，不做截图式文本放大。
- 音频调用进入 Audio cue event，不直接访问 AudioProvider native handle。
- 旧变量可映射到 compat private state、Blackboard view 或 read-only inspector field。
- 未支持 opcode/API 必须记录 frequency、script location、fallback 和 release severity。

Artemis tag mapping v1：

| Artemis tag/API | Astra output |
| --- | --- |
| `bg` / `cg` / `ev` | `VN.Background` |
| `fg` / `fgact` / `fgdel` | `VN.Character` |
| `text` / `msg` / `rt2` / `nrt` / `ruby` | `VN.Dialogue` |
| `select` / `selback` / `selnext` | `VN.Choice` |
| `bgm` / `se` / `vo` / `vostop` | `VN.Audio` |
| `extrans` / `quake` / `flash` / `colortone` / `movie` | `VN.Timeline` or Presentation effects |

Artemis VM control tags such as `jump`、`call`、`return`、`calllua`、`stop`、`wt` remain inside
the Artemis runtime. They must not be exposed as AstraVN source language features.

## 5. 外部资产

外部资产使用 `foreign-*` AssetId：

```text
foreign-bgi:/data/fg.arc#alice_idle
foreign-krkr:/fgimage/alice_happy
foreign-renpy:/images/alice happy.png
foreign-director:/DATA/CASTS/CHARS.cxt#member=alice_idle
foreign-artemis:/image/bg/bg001a.png
```

默认 mount-only：

- 外部目录只读。
- 不复制、不转换、不重打包原始资产。
- 高清替换资源若进入项目，必须作为 `native:/` 资产并带来源、授权和 sidecar。

Foreign asset policy：

```yaml
foreign_assets:
  default_policy: mount_only
  allow_copy: false
  allow_modernized_replacement: true
  replacement_root: Content/Modernized
  diagnostics:
    copied_foreign_source: blocking
    missing_license: blocking
```

## 6. 现代化 Profile

现代化配置是 Astra 文本源数据：

```yaml
id: modernization.sample
target_project: compatibility.project.sample
ui_overlay:
  dialogue_box: native:/UI/ModernDialogue
font_replacement:
  default: native:/Fonts/NotoSerifJP
filter_profiles:
  background: native:/Filters/legacy_background_clean
  character: native:/Filters/anime_line_enhance
scaling:
  mode: integer_or_fit
upscale_refs:
  - source: foreign-bgi:/cg.arc#opening
    replacement: native:/Modernized/CG/opening_4x
```

FilterGraph 必须支持 layer-aware 现代化：背景、角色、UI、文本和最终画面分别处理。

Modernization Profile 状态流：

```text
Foreign Probe
  -> Compatibility Report
  -> Modernization Draft
  -> Review
  -> Accepted Modernization Profile
  -> Runtime Overlay / Package Policy
```

现代化可修改：

- 字体 fallback。
- UI overlay。
- `native:/` 高清替换。
- FilterProfile。
- scale/layout policy。
- localization overlay。

现代化不可修改：

- foreign source package。
- legacy VM bytecode。
- native Core/Runtime/Asset/Media schema。

## 7. 调试与 Inspector

Compatibility Inspector 提供：

- 项目 probe 结果。
- 包挂载状态和缺失资源。
- external asset registry。
- VM 状态、变量、PC、调用栈或 timeline frame。
- 未支持 opcode / API 统计。
- 现代化配置验证。
- Save extension state 摘要。
- 诊断报告导出。

Inspector commands：

- remount foreign root。
- open package member read-only。
- inspect VM variables/call stack/timeline cursor。
- create modernization replacement draft。
- map missing asset to `native:/` replacement。
- export compatibility report。

所有 command 必须通过 Editor command/Review Queue 或 read-only runtime debugger。Compatibility Inspector 不能直接写 foreign root。

## 8. Save Extension State

Save extension state schema：

```yaml
extension_id: astra.compat.bgi
schema: astra.compat.bgi.save.v1
required: false
state:
  vm_pc: 12040
  call_stack: [...]
  variables_hash: sha256:...
  timeline_cursor: 22.4
  package_mounts:
    - foreign-bgi:/data
```

规则：

- Native save model 不理解 compat private state，只保存 opaque extension section 和 metadata。
- Compat 模块缺失时，可加载 native sections；compat section 保留但不可执行。
- Compat 模块版本变化必须提供 migration 或 blocking diagnostic。
- Replay 记录 legacy VM emitted event hash，定位 mapper mismatch。

## 9. Release Gate

Expansion release gate 检查：

- compat build profile explicitly enabled。
- all foreign roots are mount-only unless license and policy allow copy。
- modernization replacements are `native:/` assets with sidecar/license/review。
- unsupported opcode/API count below configured threshold or has accepted fallback。
- Save extension schema migration available。
- Compat plugins packaged eligibility matches release profile。
- Runtime/Asset/Media public APIs are not modified for compat-only assumptions。

## 10. 验收

- Mock legacy runtime fixture 可 probe、mount、step VM、emit VN presentation events。
- Compatibility Inspector 可显示 package index、VM state、missing assets、unsupported opcode 和 modernization profile diagnostics。
- Legacy save extension state 可 save/load/replay，不污染 native save sections。
- Mount-only policy 阻止复制 foreign source assets。
- Modernization replacement 通过 `native:/` sidecar、license、review 和 FilterProfile 进入 runtime。
- Compat 模块卸载后，native runtime sample 仍可 build、test、package，不需要 compat API。

## 11. 非目标

- 不默认破解、解密或绕过商业保护。
- 不承诺完整可视化编辑外部脚本。
- 不把外部项目导入为 Astra canonical source。
- 不允许兼容模块绕过 Actor、StateMachine、Presentation、Asset、Save 和 FilterGraph 边界。
- 不允许 legacy VM 或 compat package policy 反向污染 native runtime 的 Core 边界。
