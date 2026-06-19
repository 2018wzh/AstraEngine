# AstraEmu 独立工具包设计

## 1. 目标

`AstraEmu Toolkit` 是基于 AstraEngine 架构开发的独立运行工具包。它复用
Core、Platform、ModuleRuntime、Asset、Media、Runtime、Script 和 AstraVN 的运行时边界，
但不参与使用 AstraEngine 创作 NativeVN 的制作流程。

`AstraEmu` 负责：

- 自动扫描用户本地旧 VN 目录或数据文件，识别引擎家族和可用入口。
- 通过可替换 Compat Core 运行旧 VM、tag script、timeline 或私有脚本格式。
- 将可见演出输出映射到 RuntimeEvent 和 PresentationCommand。
- 使用 Headless、SDL、bgfx 或 Skia 等后端执行显示、文本、音频和滤镜。
- 提供字体替换、缩放、文本重排、layer-aware filters、HD replacement overlay、音频路由、backlog、save-state 和运行时翻译。

非目标：

- 不作为 NativeVN 项目的素材导入器。
- 不把外部游戏转换为 Astra canonical source。
- 不服务 Editor 创作工作流。
- 不修改 foreign source。
- 不破解、解密或绕过 DRM / 商业保护。

## 2. RetroArch-style 结构

`AstraEmu` 借鉴 RetroArch 的 front-end/core/content 分离，但不复制 libretro ABI。

```text
AstraEmu Manager
├─ Content Probe
├─ Compat Core Loader
├─ Backend Selection
├─ Input / Save-State / Config
├─ TextCapture Middleware
├─ Translation Provider Bridge
├─ Enhancement Profile
└─ Runtime Inspector

Compat Core
├─ Legacy package reader
├─ Legacy asset resolver
├─ Legacy VM / tag executor
├─ Legacy API mapper
├─ Save-state adapter
└─ Coverage reporter
```

运行链路：

```text
Local Game Root
  -> Probe
  -> Compat Core
  -> RuntimeEvent / PresentationCommand
  -> AstraVN semantics
  -> Media backend
```

Manager 负责窗口、输入、配置、后端选择、增强配置、翻译桥接和 core 生命周期。
Compat Core 只负责某个旧引擎家族的解析、执行、资源定位和状态捕获。

## 3. Core Contract

Compat Core 是模块化 provider，不拥有主循环，也不直接访问 renderer/audio native handle。

```cpp
class ICompatRuntimeProvider {
public:
    virtual CompatCoreDescriptor Describe() const = 0;
    virtual Result<CompatContentMatch> Probe(CompatProbeRequest, DiagnosticSink&) = 0;
    virtual Result<void> LoadContent(CompatContentMount, DiagnosticSink&) = 0;
    virtual Result<CompatStepResult> Step(RuntimeTickInput, DiagnosticSink&) = 0;
    virtual Result<LegacyVmSnapshot> CaptureSnapshot(DiagnosticSink&) = 0;
    virtual Result<void> RestoreSnapshot(LegacyVmSnapshot, DiagnosticSink&) = 0;
};
```

Descriptor 示例：

```yaml
schema: astra.emu.compat_core.v1
core_id: astra.emu.artemis
display_name: Artemis Compat Core
module_id: astra.emu.artemis
supported_engines: [artemis]
content_schemes: [foreign-artemis]
capabilities:
  script_index: true
  vm_debug: true
  text_capture: true
  save_state: true
  cold_swap: true
diagnostics_prefix: ASTRA_EMU_ARTEMIS
```

Core 输出规则：

- 图像、文本、音频、选择和转场输出必须转为 AstraVN 事件或 PresentationCommand。
- VM 控制流保留在 core 内部，不成为 AstraVN native source language。
- 私有 VM 状态只进入 `LegacyVmSnapshot`。
- 旧引擎专用 API 不反向进入 Core、Runtime、Asset 或 Media public API。

## 4. Content Probe 与本地挂载

Probe 输入：

- local root path。
- enabled core list。
- user mount policy。
- optional user profile。

Probe 输出：

- detected engine family/version。
- confidence。
- entry candidate。
- script/resource roots。
- unsupported/protected feature diagnostics。

默认挂载策略：

- foreign root 只读。
- 使用 `foreign-*:/` 引用本地内容。
- 不复制、不转换、不重写 foreign source。
- 受保护或加密数据只报告 unsupported diagnostic。

示例：

```text
foreign-artemis:/image/bg/bg001a.png
foreign-artemis:/image/fg/kot/z1/kot_z1a0000.png
foreign-artemis:/sound/bgm/bgm003.ogg
foreign-artemis:/sound/vo/asu/fem_asu_00002.ogg
```

版权边界：

- `AstraEmu` 不提供游戏数据。
- 用户必须自行提供合法取得的本地内容。
- Toolkit 不包含绕过访问控制的代码路径。
- Enhancement、translation 和 HD replacement 只作为本地 overlay 生效，不写回原始内容。

## 5. Artemis v1

Artemis v1 是首个真实目标，范围是 unpacked-directory 的可诊断运行原型。

安装态布局识别：

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

解包态布局识别：

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

启动链路：

```text
system.ini
  -> system/first.iet
  -> system/init.lua
  -> system/script.asb
  -> script/*.ast
```

v1 优先级：

- Probe：识别 `system.ini`、`system/first.iet`、`system/*.asb`、`script/*.ast` 和 media roots。
- Index：索引 `.iet`、`.asb`、`.ast`、`.ipt`、`.sli`、`.tbl`。
- Lua host：提供 Artemis `e` 对象的最小 API surface。
- Tag coverage：优先覆盖 `bg`、`fg`、`text`、`msg`、`vo`、`se`、`bgm`、`select`、`excall`、`wait`、`extrans`、`msgoff`、`quake`、`ruby`、`eval`、`movie`。
- Report：输出 unsupported tag/API/asset coverage、script location、fallback 和 severity。

Artemis 不是单一 `.ast` parser。真实运行需要 `.iet` text tag script、`.asb` binary tag script、
`.ast` Lua-table story data、system Lua modules、`e:*` host API、tag executor 和资源 resolver 协同。

## 6. Mapper 与 VN 语义

Legacy API mapper 把旧引擎调用映射为 Astra 可见演出：

| Legacy action | Astra output |
| --- | --- |
| background / CG | `VN.Background` |
| character / sprite | `VN.Character` |
| text / ruby / message | `VN.Dialogue` |
| select / branch choice | `VN.Choice` |
| bgm / se / voice | `VN.Audio` |
| transition / quake / flash / movie | `VN.Timeline` or Presentation effects |

规则：

- Mapper 不直接调用 renderer、text layout 或 audio native handle。
- 文本优先进入 Dialogue/TextLayout，不做截图式放大。
- 旧变量可保留在 compat private state，也可暴露为 read-only inspect fields。
- 未支持 opcode/API 记录 frequency、script location 和 fallback。

## 7. 后端与增强

Backend selection 通过 Astra EngineModuleSlot/provider 机制完成：

- `headless`：CI、自动化、hash 和调试。
- `sdl`：默认窗口、输入和音频路径。
- `bgfx`：跨平台 renderer 目标。
- `skia`：文本和 2D 绘制候选目标。

Enhancement Profile 是 AstraEmu 本地配置，不是 Astra project source：

```yaml
schema: astra.emu.enhancement_profile.v1
profile_id: local.artemis.default
scaling:
  mode: integer_or_fit
font_replacement:
  default: local:/fonts/NotoSerifJP.otf
filters:
  background: local:/filters/background_clean.yaml
  character: local:/filters/line_enhance.yaml
hd_replacements:
  - source: foreign-artemis:/image/bg/bg001a.png
    overlay: local:/overlays/bg001a_4x.png
translation:
  provider: local.translation.default
  mode: overlay
```

增强功能 v1：

- 整数缩放 / fit scaling。
- 字体替换和 fallback。
- 文本重排、ruby 和双语 overlay。
- 背景、角色、UI、文本、最终画面的 layer-aware filters。
- HD replacement overlay。
- BGM/SE/voice 逻辑总线。
- Backlog 和 save-state。

## 8. TextCapture 与翻译

翻译由插件化中间层实现，不写入 compat core，也不依赖平台特定注入路径。

```text
Compat Core
  -> TextCaptureEvent
  -> Text Processing
  -> Translation Provider Bridge
  -> TranslationOverlayCommand
  -> Text / UI backend
```

`TextCaptureEvent` 内容：

- source core id。
- script location or VM PC。
- speaker。
- original text。
- ruby/control metadata。
- stable text hash。

Provider 规则：

- 外部翻译 Provider 通过模块机制接入。
- Provider 可以是本地服务、离线模型或用户配置的网络服务。
- 请求、响应、缓存和错误必须进入 AstraEmu audit log。
- 翻译结果默认只显示为 overlay。
- 需要嵌入显示时必须经过 core capability 检查，失败回退 overlay。

## 9. Core 冷换

Compat Core 不做运行中二进制热替换。Manager 使用冷换语义：

```text
Pause
  -> CaptureSnapshot
  -> Quiesce tasks and backend resources
  -> Unload old core
  -> Load new core
  -> Migrate or RestoreSnapshot
  -> Resume
```

失败策略：

- 新 core 加载失败：恢复旧 core。
- snapshot schema 不兼容：保留暂停状态并显示 diagnostic。
- backend 资源重建失败：切换 headless 或停止运行。

可热载内容限于 enhancement profile、translation config、字体、滤镜、HD overlay 和 mapper rule data。

## 10. Inspector 与报告

AstraEmu Inspector 是运行时诊断面板，不是 Editor authoring 工具。

提供：

- Probe 结果。
- core descriptor 和 capability set。
- local mount 状态。
- VM PC、变量、调用栈或 timeline cursor。
- unsupported tag/API coverage。
- TextCapture 和 translation audit。
- backend capability report。
- save-state 摘要。

命令：

- remount local root。
- switch core。
- cold-swap core。
- inspect read-only member。
- toggle enhancement profile。
- toggle translation provider。
- export compatibility report。

## 11. 验收

- `AstraEmu` 可扫描本地 Artemis unpacked directory 并选择 Artemis Compat Core。
- Artemis core 可索引脚本和资源，输出 coverage report。
- Mock core 可 step、输出 VN presentation、capture/restore snapshot。
- Core cold-swap 成功时恢复 VM 状态，失败时回滚旧 core。
- TextCaptureEvent 可进入外部 translation Provider，并以 overlay 显示结果。
- Headless、SDL、bgfx、Skia backend 通过 capability report 声明可用性。
- mount-only 默认阻止修改 foreign source。


