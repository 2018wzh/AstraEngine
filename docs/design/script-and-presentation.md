# Script 与 Presentation / AstraVN 设计

状态：Target Architecture  
定位：脚本运行时、VN DSL/Lua/Graph、PresentationCommand 和 AstraVN 垂直模块的实现规格。

## 1. 目标

Script 与 Presentation 层必须让创作者能用文本、Graph、Timeline 或 Lua 编写 2D/VN 内容，
同时保证 Runtime deterministic、save/replay、debugger 和 package launch：

- ScriptRuntimeHost 支持 Native DSL、Lua 和自定义 runtime provider。
- Script 只能通过 Script API、RuntimeEvent 和 Presentation API 影响世界。
- AstraVN 提供 VN-first 的 Actor、Component、StateMachine、Event 和 Presentation Library。
- Graph/Timeline 与文本脚本共享 canonical source、debug symbol 和 runtime path。
- Legacy VM 是 expansion track ScriptRuntimeProvider，不是 native parity 前置条件。

## 2. ScriptRuntimeHost

```text
ScriptRuntimeHost
├─ Runtime provider registry
├─ Script API registry
├─ Debug hooks
├─ Snapshot/restore
├─ Hot reload validator
└─ Event bridge
```

Provider contract：

```yaml
runtime_id: astra.script.native
contract: IScriptRuntimeProvider
source_types: [.astra, .astra.yaml]
capabilities:
  debug: true
  snapshot: true
  deterministic: true
  hot_reload: state_compatible
required_services:
  - astra.runtime.event_bus
  - astra.asset_registry
  - astra.presentation.library
permissions:
  project_read: true
  project_write: false
  network: false
```

Runtime lifecycle：

```text
Register provider
  -> Compile / load source
  -> Create script instance
  -> Bind host API
  -> Start entry point
  -> Step on runtime tick/event
  -> Snapshot / restore
  -> Stop / unload
```

## 3. Script API Boundary

Allowed Script API：

- emit runtime event。
- request presentation command through Presentation Library。
- query safe Actor/Component snapshot。
- read/write permitted Blackboard values。
- schedule wait event/time/asset。
- request choice/dialogue/timeline operations。

Forbidden：

- direct renderer/audio/native handle access。
- direct Editor object access。
- direct file/network/secret access unless explicit provider permission and release profile allow。
- direct mutation of runtime-only fields, ECS entity, component private state。
- non-deterministic random or wall-clock decisions outside Runtime deterministic services。

Host API example：

```cpp
struct AstraScriptHostApi {
    AstraResult (*emit_event)(AstraScriptContext*, const AstraRuntimeEventDesc*);
    AstraResult (*request_presentation)(AstraScriptContext*, const AstraPresentationCommandDesc*);
    AstraResult (*get_actor_snapshot)(AstraScriptContext*, AstraActorId, AstraActorSnapshot*);
    AstraResult (*blackboard_get)(AstraScriptContext*, AstraBlackboardKey, AstraValue*);
    AstraResult (*blackboard_set)(AstraScriptContext*, AstraBlackboardKey, const AstraValue*);
    AstraResult (*wait_for_event)(AstraScriptContext*, AstraEventTypeId, AstraTaskToken*);
};
```

## 4. Native DSL

Native DSL source example：

```text
label opening
bg native:/Backgrounds/Room fade 0.5
show alice normal at center
say alice "早上好。"
choice:
  "一起走吧" -> route_walk
  "稍等一下" -> route_wait
```

Compile pipeline：

```text
Source
  -> Lexer / Parser
  -> AST
  -> Validation against AssetRegistry and VN schema
  -> IR command stream
  -> Debug symbols / source map
  -> Runtime script instance
```

IR command stream requirements：

- stable command id。
- source location。
- referenced AssetId / ActorId / localization key。
- command schema version。
- deterministic branch and wait semantics。

Diagnostics：

- parse error：file、line、column、expected tokens。
- missing asset：AssetId、source command、suggested Content Browser action。
- invalid actor reference：ActorId/ActorName、scene scope、suggested replacement。
- route/label missing：label、call site、available labels。
- localization missing：text key、locale、fallback policy。

## 5. Lua Runtime

Lua runtime is optional but native parity target includes sandboxed host support：

- deterministic random service。
- restricted standard library。
- no raw filesystem/network by default。
- host API binding only through registered Script API。
- snapshot of Lua VM state or equivalent deterministic continuation state。
- debug hook for breakpoint、step、call stack、local variables。

Release Gate：

- blocks Lua runtime if provider lacks packaged eligibility。
- blocks scripts using forbidden host APIs。
- verifies snapshot support for save/replay-required scripts。

## 6. Graph And Timeline Source

Graph source compiles to the same IR/event path as text DSL：

```text
Graph Node
  -> IR command
  -> RuntimeEvent
  -> StateMachine / PresentationCommand
```

Timeline source emits deterministic timeline events and presentation state：

- cursor enters save/replay。
- active tracks enter save/replay。
- emitted events use RuntimeEvent sequence。
- preview uses same compiler and PresentationExtractor as PIE/package。

Graph/Timeline editors must never maintain a separate runtime model. They can hold editor-only layout metadata, but execution source is canonical graph/timeline data plus compiled debug symbols.

## 7. Presentation Command

PresentationCommand schema：

```yaml
command_id: presentation:/frame/120/text_001
kind: astra.presentation.text.start
sequence: 12045
source:
  script: native:/Scripts/opening
  location: Scripts/opening.astra:4
target:
  actor: actor:/systems/dialogue
payload_schema: astra.presentation.text.start.v1
payload:
  speaker: actor:/characters/alice
  text_key: loc:/opening/alice_001
  typewriter:
    speed: 32
```

Presentation command categories：

- sprite/background/character。
- text/dialogue/backlog。
- choice/ui。
- audio。
- camera。
- timeline/animation。
- filter/effects。

Commands are extracted from runtime state and events. Media executes commands but does not own story state.

## 8. AstraVN Module

AstraVN public event categories：

- `VN.Background`
- `VN.Character`
- `VN.Dialogue`
- `VN.Choice`
- `VN.Audio`
- `VN.Timeline`
- `VN.Filter`
- `VN.Camera`

Preset actors：

- `SceneActor`
- `StoryDirectorActor`
- `DialogueSystemActor`
- `ChoiceSystemActor`
- `AudioSystemActor`
- `FilterSystemActor`
- `CharacterActor`
- `CameraActor`

Preset components：

- character profile。
- emotion。
- dialogue participant。
- choice list。
- audio cue。
- camera。
- timeline。
- filter profile。

Preset state machines：

- Dialogue。
- Choice。
- CharacterPresentation。
- Background。
- Audio。
- Timeline。
- FilterProfile。

## 8.1 Shared VN Semantics For Compat Runtimes

Compat runtimes reuse AstraVN output semantics, not AstraVN input languages. Artemis、BGI、Kirikiri
or other legacy runtimes may keep private parsers and VM state, but their visible story output must
cross the same VN event and PresentationCommand boundary as native AstraVN content.

Recommended chain：

```text
ArtemisRuntime
  -> ArtemisTagExecutor
  -> ArtemisApiMapper
  -> AstraVN Event / RuntimeEvent
  -> PresentationCommand
  -> Media
```

Shared：

- VN event categories：Background、Character、Dialogue、Choice、Audio、Timeline、Filter、Camera。
- Preset systems：DialogueSystem、ChoiceSystem、AudioSystem、FilterSystem、Character、Camera。
- PresentationCommand categories：sprite/background/character、text/dialogue/backlog、choice/ui、audio、timeline/effects、filter。
- Dialogue semantics：speaker、text body、ruby annotation、voice binding、backlog entry、read/skip hooks。
- Runtime hooks：choice result、auto/skip/click advance、timeline wait、filter profile、save/replay extension metadata。

Not shared：

- Artemis input formats：`system.ini`、`.iet`、`.asb`、`.ast`、system Lua modules。
- Artemis VM control flow：`jump`、`call`、`return`、`calllua`、`stop`、`wt`。
- Artemis host API：`e:tag`、`e:file`、`e:include`、`e:var`、surface/cache/input helpers。
- Artemis magic path and package resolver。
- Artemis system UI behavior for save/load、config、extra、quickjump unless mapped through explicit compat UI commands。

Compat-specific details stay in compat private state or command payloads when required for diagnostics,
but native AstraVN DSL、Graph、Timeline and Lua must not depend on Artemis-only semantics.

## 9. Debugger And Hot Reload

Debugger hooks：

- breakpoint by source location、command id、graph node、timeline key。
- step into/over command。
- inspect variables、Blackboard、Actor snapshot、current command。
- show event and presentation command generated by current script command。

Hot reload policy：

- parse and validate new source。
- compile new IR。
- compare old/new command ids and active continuation state。
- if compatible, migrate continuation。
- if incompatible, rollback and emit diagnostics。

## 10. Save / Replay

Script snapshot must include：

- active runtime id and module version。
- current command/label/node/timeline cursor。
- variables and call stack。
- pending waits and scheduler task ids。
- deterministic random state。
- emitted committed AI output references if script consumed them。

Replay must not re-run provider calls or depend on Editor source files after package build. It uses cooked script artifacts, package manifest and committed runtime data.

## 11. 验收

- Native DSL and Lua can drive the same VN scene and produce equivalent RuntimeEvent / PresentationCommand sequences where authored to do so。
- Graph/Timeline authoring compiles to the same runtime path as text DSL。
- Script debugger can break、step、inspect、resume in PIE using the same RuntimeWorld as package。
- Script hot reload rolls back on incompatible state。
- Save/load restores active label/node/timeline/script variables/waits。
- AstraVN sample runs dialogue、choice、character、background、audio、timeline、filter、camera with deterministic replay。
