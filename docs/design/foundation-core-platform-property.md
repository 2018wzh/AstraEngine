# Foundation：Core / Platform / Property 设计

状态：Phase 1 Foundation Implemented / Target Architecture  
定位：Astra 的基础类型、诊断、配置、序列化、平台抽象和轻量属性系统。它们支撑 Runtime、Editor、CLI、MCP、插件和 Release Gate，但不包含 VN、AI、Legacy 或 Editor 语义。

Phase 1 implementation note：当前工作树已实现 `AstraCore`、`AstraPlatform`、`AstraModuleRuntime` 和 `AstraPropertySystem` 的 production-ready Foundation gate slice。它覆盖 diagnostics、diagnostic code registry、release profile policy、`spdlog`-backed structured logging、error reporting、profiling markers、config layering/profile hash、stable id、versioned document/migration、unknown-field policy、build info、headless platform services、opaque dynamic library handles、thread/file-watch/crash smoke、SDL private backend compile path、module descriptor/service/provider release-gate checks、Property descriptors、nested JSON Schema generation、schema version graph、write policy、validation defaults、diff/audit output 和 migration helpers。future Editor/MCP consumers、完整 input/window feature set、save/package critical migration policy、trace export 和完整 crash bundle 仍是后续阶段。

## 1. 目标

Foundation 必须提供 UE-class 2D runtime 所需的基础工程能力：

- stable ids、versioned serialization、migration、diagnostics、logging、config、time、path。
- public ABI 不暴露 STL ownership、C++ object ownership、SDL/OS/native handle。
- Platform 提供 headless 和 SDL-backed backend，但 SDL 限制在 private implementation。
- PropertySystem 支撑 schema、Inspector、serialization、AI review、MCP field editing 和 Release Gate。
- 所有基础服务能被 Runtime、Editor、CLI 和 MCP 以同一 diagnostics 格式消费。

非目标：

- 不实现 UE UObject/UHT/GC。
- 不让 Core 依赖 Platform、Runtime、Editor、MCP、AI、Lua、VN、Compat、renderer 或 audio。

## 2. Core Types And Result

基础类型策略：

```text
integer      fixed width: u8/u16/u32/u64/i8/i16/i32/i64
string       UTF-8, no implicit locale conversion
path         normalized project/package/user/cache path
view         span/string_view style non-owning view
result       Result<T, ErrorCode> or C ABI result code
id           typed stable id, parseable and hashable
time         monotonic, game, fixed-step, real-time
```

C ABI 只使用：

- fixed-width scalar。
- UTF-8 `char*` with explicit length。
- POD descriptor。
- opaque handle。
- callback function pointer。
- explicit allocator or host-owned buffer。

禁止跨 ABI：

- STL container ownership。
- `std::string` ownership。
- C++ Actor/Component pointer。
- SDL/OS/GPU/audio handle。
- Editor widget pointer。

## 3. Diagnostics

Diagnostic packet：

```yaml
code: ASTRA_ASSET_001
category: asset.dependency
severity: blocking
message: Missing asset dependency
source:
  file: Content/Scripts/opening.astra
  line: 12
  column: 5
objects:
  - kind: AssetId
    id: native:/Characters/Alice/Normal
context:
  package_profile: Release
suggested_fixes:
  - action: open_content_browser
  - action: replace_asset_ref
```

Severity：

- `info`
- `warning`
- `error`
- `blocking`
- `fatal`

Rules：

- Release Gate 只根据 `blocking/fatal` 和 profile policy 阻止发布。
- Runtime fatal 必须包含 last log、build info、frame index、thread id、optional crash bundle path。
- Editor 和 CLI 必须显示同一 diagnostic code，不重新解释成不兼容格式。
- MCP tools 返回 diagnostics array，而不是自由文本错误。

## 4. Logging

Log event：

```yaml
time: monotonic_ns
channel: runtime.event
severity: info
message: Event dispatched
fields:
  frame: 120
  event_type: astra.vn.dialogue.say_requested
  actor: actor:/systems/dialogue
```

Sinks：

- console。
- file with rotation。
- Editor Output Log。
- runtime crash bundle。
- test capture。
- MCP operation response。

Log 不替代 diagnostics。Diagnostics 是可机器处理的错误/警告；Log 是时间序列观测。

## 5. Config

Config scopes：

```text
Engine default
  -> Platform default
  -> Project config
  -> Runtime profile
  -> Release profile
  -> User/editor local override
  -> Command line
```

Project config example：

```yaml
project:
  id: native.project.sample
  schema: astra.project.v1
runtime:
  fixed_step_hz: 60
  deterministic: true
release:
  profile: deterministic
  allow_runtime_ai: false
engine_modules:
  selections:
    astra.renderer2d: astra.renderer2d.default
```

Rules：

- Cook/package 使用 project config + release profile，不使用 editor user override。
- Runtime package manifest stores resolved config hash。
- Config migration is schema-versioned。
- Secrets never live in project config; SecretProvider owns them.

## 6. Serialization And Migration

Versioned document header：

```yaml
schema: astra.component.character_profile.v1
version: 1
object_id: component:/characters/alice/profile
```

Migration registry：

```text
schema id
from version
to version
migration function
unknown field policy
diagnostic code
```

Unknown field policy：

- `preserve` for forward-compatible source edits。
- `warn` for deprecated optional fields。
- `error` for runtime/save/package critical schema。
- `drop` only for explicit migration rules。

Serialization targets：

- source YAML/JSON。
- binary cooked artifact。
- save section。
- replay event stream。
- diagnostics payload。

## 7. Stable Id Framework

Stable IDs：

- `TypeId`
- `PropertyId`
- `AssetId`
- `ActorId`
- `ComponentId`
- `EventTypeId`
- `TaskId`
- `StateMachineId`
- `ProviderId`

Rules：

- ID parsing normalizes case and path policy where applicable。
- IDs are never raw memory addresses or ECS entities。
- Missing or duplicate IDs produce diagnostics with object location。
- Generated IDs must be deterministic from canonical source or explicitly persisted.

## 8. Platform Services

Platform public services：

```text
WindowService
InputService
FileSystemService
DynamicLibraryService
ThreadService
TimerService
CrashService
Clipboard/Cursor/Display service
```

Backend requirements：

- Headless backend：CI、server-style runtime validation、package smoke、replay。
- SDL-backed backend：window/input/timer/filesystem/dynamic library, SDL private only。
- Future backend：must conform to same public descriptors and diagnostics。

FileSystem mounts：

```text
project:/       read/write source project
package:/       read-only cooked package
user:/          save/config user data
cache:/         DerivedDataCache
foreign-*/      read-only external mount by policy
```

Threading rules：

- Runtime main-thread command queue is explicit。
- Worker tasks must declare tags and shutdown behavior。
- Module unload waits for owned tasks or reports blocking diagnostic。
- Public API does not expose native thread handles.

Crash service：

- build info。
- last diagnostics。
- last N logs。
- frame index and runtime state summary。
- optional minidump path。
- package/project hash。

## 9. PropertySystem

TypeDescriptor example：

```yaml
type_id: astra.vn.character_profile
kind: struct
version: 1
properties:
  - id: display_name
    type: localized_text
    default: loc:/characters/unknown
    flags: [creator_editable]
    inspector:
      category: Character
      order: 10
  - id: route_role
    type: enum:astra.vn.route_role
    flags: [requires_review]
    validation:
      required: true
```

Property kinds：

- scalar。
- enum。
- localized text。
- asset ref。
- struct。
- array。
- map。
- tagged union。

Flags：

- `ai_editable`
- `tool_generated`
- `read_only`
- `requires_review`
- `runtime_only`
- `editor_only`
- `release_sensitive`

Consumers：

- JSON Schema generation。
- Inspector。
- serialization。
- source diff。
- Review Queue。
- MCP field editing。
- Release Gate。

## 10. Validation And Release Gate

Foundation release checks：

- schema id and version valid。
- migration path exists for source/save/package。
- unknown fields follow policy。
- diagnostics codes registered。
- platform backend packaged eligible。
- no public ABI forbidden type。
- config hash and build info present。
- Property flags respected by AI/MCP/editor writes。

## 11. Tests

Required tests：

- diagnostics packet serialization and severity policy。
- config layering and command-line override。
- stable id parse/normalize/hash。
- schema generation for nested struct/array/map/tagged union。
- migration preserve/warn/error/drop behavior。
- headless platform filesystem/timer/thread/dynamic library smoke。
- SDL public header isolation check。
- C ABI forbidden type scan。

## 12. 验收

- Core builds without Platform、Runtime、Editor、AI、VN、Lua、Compat dependencies。
- Diagnostics from CLI、Runtime、Editor、MCP and Release Gate share the same packet schema。
- Headless backend can run validation/replay/package smoke without window。
- PropertySystem can drive Inspector metadata、JSON Schema、serialization、AI review and MCP field editing。
- Public ABI headers expose only approved C ABI types and opaque handles。

Phase 1 evidence：

- `AstraCore` builds without Platform、SDL、Lua、VN、AI、Editor、renderer、audio 或 Compat。
- `AstraPlatform` headless tests cover thread dispatch、timer、crash packet；SDL types remain in private implementation.
- `AstraPropertySystem` tests cover JSON Schema、required/default validation 和 migration helper。
- `AstraPhaseTests` includes public header forbidden-token isolation checks。
