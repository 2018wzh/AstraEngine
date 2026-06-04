# 旧 VN 引擎模拟器与现代化设计

## 1. 目标

Compatibility Layer 的目标是支持现有 VN 引擎项目或游戏包的运行、诊断、调试和现代化。它不是 Import 工具，不要求把外部脚本转换为 Astra DSL，也不默认复制外部原始资产。

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

## 4. API Mapper

Legacy API Mapper 把旧引擎调用映射为 Astra 事件或 Presentation Command：

- 图像显示 -> Character/Background/Layer event。
- 文本输出 -> Dialogue event。
- 音频 -> Audio cue event。
- 选择 -> Choice event。
- 转场 -> Timeline 或 FilterProfile event。
- 系统变量 -> Blackboard 或 compat state。

Mapper 不直接调用 Renderer2D 或 AudioCore native handle。

## 5. 外部资产

外部资产使用 `foreign-*` AssetId：

```text
foreign-bgi:/data/fg.arc#alice_idle
foreign-krkr:/fgimage/alice_happy
foreign-renpy:/images/alice happy.png
foreign-director:/DATA/CASTS/CHARS.cxt#member=alice_idle
```

默认 mount-only：

- 外部目录只读。
- 不复制、不转换、不重打包原始资产。
- 高清替换资源若进入项目，必须作为 `native:/` 资产并带来源、授权和 sidecar。

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

## 8. 非目标

- 不默认破解、解密或绕过商业保护。
- 不承诺完整可视化编辑外部脚本。
- 不把外部项目导入为 Astra canonical source。
- 不允许兼容模块绕过 Actor、StateMachine、Presentation、Asset、Save 和 FilterGraph 边界。
